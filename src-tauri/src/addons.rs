//! Addon host (docs/addons.md): loads enabled addons as Extism plugins,
//! exposes the host API with permission checks, and calls their hooks.
//!
//! Plugins run on blocking threads with a time limit per call and a memory
//! cap. HTTP is Extism's built-in client, limited to the manifest's
//! `allowed_hosts` and only with the `network` permission. Host functions
//! read and write through `time-core`, so addon writes are validated and
//! change-logged like any other write.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use extism::convert::Json;
use extism::{CurrentPlugin, Manifest, Plugin, PluginBuilder, UserData, Val, Wasm, PTR};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;
use time_addon_sdk as sdk;
use time_core::addon::{AddonManifest, Hook, Permission, SettingType};
use time_core::events::DomainEvent;
use time_core::model::{EntryInput, EntrySource};
use time_core::summary::Range;
use time_core::AddonRecord;

use crate::events::DataKind;
use crate::state::AppState;

/// Longest a single hook call may run.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// Quick-switch suggestions must not hold up typing.
const SUGGEST_TIMEOUT: Duration = Duration::from_millis(800);
/// 64 KiB pages: 1024 = 64 MiB per plugin.
const MEMORY_PAGES: u32 = 1024;

pub const MANIFEST_FILE: &str = "addon.toml";

struct Loaded {
    manifest: AddonManifest,
    plugin: Arc<Mutex<Plugin>>,
}

/// Addon state of the running app.
#[derive(Default)]
pub struct Addons {
    loaded: Mutex<HashMap<String, Arc<Loaded>>>,
    /// Last error per addon, shown on the Addons page.
    errors: Mutex<HashMap<String, String>>,
    /// Last `schedule.tick` per addon (unix ms).
    last_tick: Mutex<HashMap<String, i64>>,
}

/// Folder with one sub-folder per installed addon.
pub fn addons_dir(app: &AppHandle) -> std::io::Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(std::io::Error::other)?
        .join("addons");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// The installed manifest of an addon.
pub fn read_manifest(dir: &Path) -> Result<AddonManifest, String> {
    let src =
        std::fs::read_to_string(dir.join(MANIFEST_FILE)).map_err(|e| format!("{MANIFEST_FILE}: {e}"))?;
    AddonManifest::parse(&src).map_err(|e| e.to_string())
}

// --------------------------------------------------------------------------
// Host functions
// --------------------------------------------------------------------------

/// Shows a desktop notification (title, body).
pub type NotifyFn = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// What the host functions use besides the core; the app passes Tauri
/// based ones, tests their own.
#[derive(Clone)]
pub struct HostServices {
    pub core: Arc<time_core::Core>,
    pub notify: NotifyFn,
    /// Called after an addon created an entry, so windows reload.
    pub entries_changed: Arc<dyn Fn() + Send + Sync>,
}

impl HostServices {
    fn for_app(app: &AppHandle) -> Self {
        let notify_app = app.clone();
        let changed_app = app.clone();
        Self {
            core: app.state::<AppState>().core.clone(),
            notify: Arc::new(move |title, body| {
                let _ = notify_app.notification().builder().title(title).body(body).show();
            }),
            entries_changed: Arc::new(move || {
                let app = changed_app.clone();
                tauri::async_runtime::spawn(async move {
                    crate::commands::changed(&app, &[DataKind::Entry]).await;
                });
            }),
        }
    }
}

/// What a plugin's host calls run with.
struct HostCtx {
    services: HostServices,
    addon_id: String,
    addon_name: String,
    granted: Vec<Permission>,
}

impl HostCtx {
    fn require(&self, p: Permission) -> Result<(), extism::Error> {
        if self.granted.contains(&p) {
            Ok(())
        } else {
            let name = serde_json::to_value(p)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string));
            Err(extism::Error::msg(format!(
                "permission '{}' was not granted",
                name.unwrap_or_default()
            )))
        }
    }

    fn core(&self) -> Arc<time_core::Core> {
        self.services.core.clone()
    }
}

/// Converts between the core's and the SDK's JSON-compatible types.
fn convert<A: Serialize, B: DeserializeOwned>(value: A) -> Result<B, extism::Error> {
    Ok(serde_json::from_value(serde_json::to_value(value)?)?)
}

/// Entries for addons, with local start and end in `tz`.
pub fn sdk_entries(entries: Vec<time_core::model::TimeEntry>, tz: chrono_tz::Tz) -> Vec<sdk::Entry> {
    let local = |ms: i64| {
        chrono::DateTime::from_timestamp_millis(ms)
            .map(|d| d.with_timezone(&tz).format("%Y-%m-%dT%H:%M").to_string())
    };
    entries
        .into_iter()
        .filter_map(|e| {
            let (start, end) = (local(e.start_at), e.end_at.and_then(local));
            let mut out: sdk::Entry = serde_json::from_value(serde_json::to_value(e).ok()?).ok()?;
            out.local_start = start;
            out.local_end = end;
            Some(out)
        })
        .collect()
}

/// Host calls run on the plugin's blocking thread; core calls are async.
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tauri::async_runtime::block_on(f)
}

type HostResult = Result<(), extism::Error>;

/// Runs a host function: decodes the JSON input, checks the permission,
/// calls `f` and writes the JSON output.
fn host<I, O>(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    user_data: UserData<HostCtx>,
    permission: Option<Permission>,
    f: impl FnOnce(&HostCtx, I) -> Result<O, extism::Error>,
) -> HostResult
where
    I: DeserializeOwned,
    O: Serialize,
{
    let Json(input): Json<I> = plugin.memory_get_val(&inputs[0])?;
    let ctx = user_data.get()?;
    let ctx = ctx.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = permission {
        ctx.require(p)?;
    }
    let output = f(&ctx, input)?;
    let handle = plugin.memory_new(Json(output))?;
    outputs[0] = plugin.memory_to_val(handle);
    Ok(())
}

fn time_list_entries(p: &mut CurrentPlugin, i: &[Val], o: &mut [Val], u: UserData<HostCtx>) -> HostResult {
    host(
        p,
        i,
        o,
        u,
        Some(Permission::EntriesRead),
        |ctx, range: sdk::Range| {
            let entries = block_on(ctx.core().list_entries(Range {
                from: range.from,
                to: range.to,
            }))?;
            convert::<_, Vec<sdk::Entry>>(entries)
        },
    )
}

fn time_list_projects(p: &mut CurrentPlugin, i: &[Val], o: &mut [Val], u: UserData<HostCtx>) -> HostResult {
    host(p, i, o, u, Some(Permission::ProjectsRead), |ctx, (): ()| {
        convert::<_, Vec<sdk::Project>>(block_on(ctx.core().list_projects(true))?)
    })
}

fn time_list_tags(p: &mut CurrentPlugin, i: &[Val], o: &mut [Val], u: UserData<HostCtx>) -> HostResult {
    host(p, i, o, u, Some(Permission::TagsRead), |ctx, (): ()| {
        convert::<_, Vec<sdk::Tag>>(block_on(ctx.core().list_tags())?)
    })
}

fn time_list_cost_centers(
    p: &mut CurrentPlugin,
    i: &[Val],
    o: &mut [Val],
    u: UserData<HostCtx>,
) -> HostResult {
    host(p, i, o, u, Some(Permission::CostCentersRead), |ctx, (): ()| {
        convert::<_, Vec<sdk::CostCenter>>(block_on(ctx.core().list_cost_centers())?)
    })
}

fn time_create_entry(p: &mut CurrentPlugin, i: &[Val], o: &mut [Val], u: UserData<HostCtx>) -> HostResult {
    host(
        p,
        i,
        o,
        u,
        Some(Permission::EntriesWrite),
        |ctx, n: sdk::NewEntry| {
            let input = EntryInput {
                activity_id: n.activity_id,
                project_id: n.project_id,
                start_at: n.start_at,
                end_at: Some(n.end_at),
                tz: None,
                note: n.note,
                billable: n.billable,
                tag_ids: n.tag_ids,
                cost_center_id: n.cost_center_id,
            };
            // Recorded as the addon's, so its own events are not sent back to it.
            let entry = block_on(ctx.core().create_entry_from(
                input,
                EntrySource::Addon,
                Some(ctx.addon_id.clone()),
            ))?;
            (ctx.services.entries_changed)();
            convert::<_, sdk::Entry>(entry)
        },
    )
}

fn time_notify(p: &mut CurrentPlugin, i: &[Val], o: &mut [Val], u: UserData<HostCtx>) -> HostResult {
    host(
        p,
        i,
        o,
        u,
        Some(Permission::Notify),
        |ctx, n: sdk::Notification| {
            // The addon's name in the title, so users know where it comes from.
            let title = format!("{} · {}", ctx.addon_name, n.title);
            (ctx.services.notify)(&title, &n.body);
            Ok(())
        },
    )
}

fn time_kv_get(p: &mut CurrentPlugin, i: &[Val], o: &mut [Val], u: UserData<HostCtx>) -> HostResult {
    host(p, i, o, u, None, |ctx, key: String| {
        Ok(block_on(ctx.core().addon_kv_get(&ctx.addon_id, &key))?)
    })
}

fn time_kv_set(p: &mut CurrentPlugin, i: &[Val], o: &mut [Val], u: UserData<HostCtx>) -> HostResult {
    host(p, i, o, u, None, |ctx, w: sdk::KvWrite| {
        Ok(block_on(ctx.core().addon_kv_set(
            &ctx.addon_id,
            &w.key,
            w.value.as_deref(),
        ))?)
    })
}

fn time_kv_keys(p: &mut CurrentPlugin, i: &[Val], o: &mut [Val], u: UserData<HostCtx>) -> HostResult {
    host(p, i, o, u, None, |ctx, prefix: String| {
        Ok(block_on(ctx.core().addon_kv_keys(&ctx.addon_id, &prefix))?)
    })
}

// --------------------------------------------------------------------------
// Loading and calling
// --------------------------------------------------------------------------

/// Settings as Extism config: saved values over manifest defaults.
fn config(manifest: &AddonManifest, saved: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    manifest
        .settings
        .iter()
        .filter_map(|field| {
            let value = saved
                .get(&field.key)
                .cloned()
                .or_else(|| field.default.clone())
                .or_else(|| (field.kind == SettingType::Boolean).then(|| "false".to_string()))?;
            Some((field.key.clone(), value))
        })
        .collect()
}

fn build_plugin(
    services: &HostServices,
    dir: &Path,
    manifest: &AddonManifest,
    record: &AddonRecord,
) -> Result<Plugin, String> {
    let mut wasm_manifest = Manifest::new([Wasm::file(dir.join(&manifest.wasm))])
        .with_timeout(CALL_TIMEOUT)
        .with_memory_max(MEMORY_PAGES)
        .with_config(config(manifest, &record.settings).into_iter());
    if record.granted.contains(&Permission::Network) {
        wasm_manifest = wasm_manifest.with_allowed_hosts(manifest.allowed_hosts.clone().into_iter());
    }
    let data = UserData::new(HostCtx {
        services: services.clone(),
        addon_id: manifest.id.clone(),
        addon_name: manifest.name.clone(),
        granted: record.granted.clone(),
    });
    PluginBuilder::new(wasm_manifest)
        .with_wasi(false)
        .with_function(
            sdk::host_fns::LIST_ENTRIES,
            [PTR],
            [PTR],
            data.clone(),
            time_list_entries,
        )
        .with_function(
            sdk::host_fns::LIST_PROJECTS,
            [PTR],
            [PTR],
            data.clone(),
            time_list_projects,
        )
        .with_function(
            sdk::host_fns::LIST_TAGS,
            [PTR],
            [PTR],
            data.clone(),
            time_list_tags,
        )
        .with_function(
            sdk::host_fns::LIST_COST_CENTERS,
            [PTR],
            [PTR],
            data.clone(),
            time_list_cost_centers,
        )
        .with_function(
            sdk::host_fns::CREATE_ENTRY,
            [PTR],
            [PTR],
            data.clone(),
            time_create_entry,
        )
        .with_function(sdk::host_fns::NOTIFY, [PTR], [PTR], data.clone(), time_notify)
        .with_function(sdk::host_fns::KV_GET, [PTR], [PTR], data.clone(), time_kv_get)
        .with_function(sdk::host_fns::KV_SET, [PTR], [PTR], data.clone(), time_kv_set)
        .with_function(sdk::host_fns::KV_KEYS, [PTR], [PTR], data, time_kv_keys)
        .build()
        .map_err(|e| format!("could not load {}: {e}", manifest.wasm))
}

impl Addons {
    fn set_error(&self, id: &str, error: Option<String>) {
        let mut errors = self.errors.lock().unwrap_or_else(|e| e.into_inner());
        match error {
            Some(e) => {
                eprintln!("addon {id}: {e}");
                errors.insert(id.to_string(), e);
            }
            None => {
                errors.remove(id);
            }
        }
    }

    pub fn error(&self, id: &str) -> Option<String> {
        self.errors
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
    }

    pub fn is_loaded(&self, id: &str) -> bool {
        self.loaded
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(id)
    }

    fn loaded(&self) -> Vec<Arc<Loaded>> {
        self.loaded
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect()
    }

    pub fn unload(&self, id: &str) {
        self.loaded.lock().unwrap_or_else(|e| e.into_inner()).remove(id);
    }

    /// (Re)loads an addon if it is enabled, otherwise unloads it.
    pub fn reload(&self, app: &AppHandle, record: &AddonRecord) {
        self.unload(&record.id);
        if !record.enabled {
            self.set_error(&record.id, None);
            return;
        }
        let result = addons_dir(app).map_err(|e| e.to_string()).and_then(|dir| {
            let dir = dir.join(&record.id);
            let manifest = read_manifest(&dir)?;
            let plugin = build_plugin(&HostServices::for_app(app), &dir, &manifest, record)?;
            Ok(Loaded {
                manifest,
                plugin: Arc::new(Mutex::new(plugin)),
            })
        });
        match result {
            Ok(loaded) => {
                self.set_error(&record.id, None);
                self.loaded
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(record.id.clone(), Arc::new(loaded));
            }
            Err(e) => self.set_error(&record.id, Some(e)),
        }
    }

    /// Calls an exported function with JSON input; errors are recorded.
    async fn call(
        &self,
        addon: &Arc<Loaded>,
        function: &'static str,
        input: String,
        timeout: Duration,
    ) -> Option<String> {
        let plugin = addon.plugin.clone();
        let task = tauri::async_runtime::spawn_blocking(move || {
            let mut plugin = plugin.lock().unwrap_or_else(|e| e.into_inner());
            if !plugin.function_exists(function) {
                return Err(format!("the addon does not export `{function}`"));
            }
            plugin
                .call::<&str, String>(function, &input)
                .map_err(|e| format!("{function}: {}", e.root_cause()))
        });
        let id = &addon.manifest.id;
        match tokio::time::timeout(timeout, task).await {
            Ok(Ok(Ok(out))) => Some(out),
            Ok(Ok(Err(e))) => {
                self.set_error(id, Some(e));
                None
            }
            Ok(Err(e)) => {
                self.set_error(id, Some(format!("{function} crashed: {e}")));
                None
            }
            Err(_) => {
                self.set_error(
                    id,
                    Some(format!("{function} took longer than {} ms", timeout.as_millis())),
                );
                None
            }
        }
    }

    /// Sends a domain event to every enabled addon that declared its hook.
    pub async fn dispatch(&self, event: &DomainEvent) {
        let hook = event.hook();
        // An addon does not get events about entries it created itself,
        // which would invite endless loops.
        let created_by = match event {
            DomainEvent::EntryCreated { entry } | DomainEvent::EntryUpdated { entry } => {
                entry.source_ref.as_deref()
            }
            _ => None,
        };
        let addons: Vec<_> = self
            .loaded()
            .into_iter()
            .filter(|a| a.manifest.hooks.iter().any(|h| h.as_str() == hook))
            .filter(|a| created_by != Some(a.manifest.id.as_str()))
            .collect();
        if addons.is_empty() {
            return;
        }
        let Ok(input) = serde_json::to_string(event) else {
            return;
        };
        for addon in addons {
            self.call(&addon, "on_event", input.clone(), CALL_TIMEOUT).await;
        }
    }

    /// `schedule.tick` for addons whose interval has passed.
    pub async fn tick(&self, now: i64) {
        for addon in self.loaded() {
            let Some(minutes) = addon.manifest.schedule_minutes else {
                continue;
            };
            if !addon.manifest.hooks.contains(&Hook::ScheduleTick) {
                continue;
            }
            let due = {
                let mut last = self.last_tick.lock().unwrap_or_else(|e| e.into_inner());
                let previous = *last.get(&addon.manifest.id).unwrap_or(&i64::MIN);
                let due = now.saturating_sub(previous) >= i64::from(minutes) * 60_000;
                if due {
                    last.insert(addon.manifest.id.clone(), now);
                }
                due
            };
            if due {
                let input = serde_json::to_string(&sdk::Tick { now }).unwrap_or_default();
                self.call(&addon, "on_schedule_tick", input, CALL_TIMEOUT).await;
            }
        }
    }

    /// Export formats of enabled addons: (addon id, addon name, format).
    pub fn export_formats(&self) -> Vec<(String, String, time_core::addon::ExportFormatDef)> {
        let mut out: Vec<_> = self
            .loaded()
            .into_iter()
            .filter(|a| a.manifest.hooks.contains(&Hook::ReportExport))
            .flat_map(|a| {
                a.manifest
                    .export_formats
                    .iter()
                    .map(|f| (a.manifest.id.clone(), a.manifest.name.clone(), f.clone()))
                    .collect::<Vec<_>>()
            })
            .collect();
        out.sort_by(|a, b| a.2.name.cmp(&b.2.name));
        out
    }

    /// Runs an addon export; returns the file contents.
    pub async fn export(&self, addon_id: &str, request: &sdk::ExportRequest) -> Result<String, String> {
        let addon = self
            .loaded()
            .into_iter()
            .find(|a| a.manifest.id == addon_id && a.manifest.hooks.contains(&Hook::ReportExport))
            .ok_or_else(|| format!("addon {addon_id} is not enabled"))?;
        let input = serde_json::to_string(request).map_err(|e| e.to_string())?;
        let out = self
            .call(&addon, "export_report", input, CALL_TIMEOUT)
            .await
            .ok_or_else(|| self.error(addon_id).unwrap_or_else(|| "export failed".into()))?;
        let result: sdk::ExportResult =
            serde_json::from_str(&out).map_err(|e| format!("export_report returned invalid JSON: {e}"))?;
        Ok(result.content)
    }

    /// Quick-switch options from addons; slow or failing addons are skipped.
    pub async fn suggest(&self, query: &str) -> Vec<(String, sdk::Suggestion)> {
        let input = serde_json::to_string(&sdk::SuggestRequest { query: query.into() }).unwrap_or_default();
        let mut out = Vec::new();
        for addon in self.loaded() {
            if !addon.manifest.hooks.contains(&Hook::QuickswitchSuggest) {
                continue;
            }
            if let Some(json) = self
                .call(&addon, "quickswitch_suggest", input.clone(), SUGGEST_TIMEOUT)
                .await
            {
                match serde_json::from_str::<Vec<sdk::Suggestion>>(&json) {
                    Ok(list) => out.extend(list.into_iter().take(20).map(|s| (addon.manifest.id.clone(), s))),
                    Err(e) => self.set_error(&addon.manifest.id, Some(format!("quickswitch_suggest: {e}"))),
                }
            }
        }
        out
    }
}

/// Loads enabled addons and forwards domain events to them.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let addons = app.state::<Addons>();
        match state.core.list_addons().await {
            Ok(records) => {
                for record in records {
                    addons.reload(&app, &record);
                }
            }
            Err(err) => eprintln!("could not list addons: {err}"),
        }
        let mut events = state.core.subscribe();
        loop {
            match events.recv().await {
                Ok(event) => addons.dispatch(&event).await,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    eprintln!("addons missed {n} events");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

/// Runs the example addons from `addons/` (build them first with
/// `addons/build.sh csv-export` and `addons/build.sh recent-notes`).
#[cfg(test)]
mod tests {
    use time_core::clock::ManualClock;
    use time_core::model::ProjectInput;

    use super::*;

    const HOUR: i64 = 3_600_000;
    /// 2026-10-05 08:00 UTC.
    const T0: i64 = 1_791_187_200_000;

    struct Fixture {
        addons: Addons,
        core: Arc<time_core::Core>,
        notifications: Arc<Mutex<Vec<(String, String)>>>,
        _dir: PathBuf,
    }

    /// Loads a built example addon with `granted`; `None` if not built.
    async fn load(package: &str, granted: &[Permission]) -> Option<Fixture> {
        let zip = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../addons/dist")
            .join(package);
        if !zip.exists() {
            eprintln!("skipped: {} not built (run addons/build.sh)", zip.display());
            return None;
        }
        let core = Arc::new(
            time_core::Core::open_in_memory(Arc::new(ManualClock::new(T0)))
                .await
                .unwrap(),
        );
        core.set_default_tz("UTC").unwrap();
        let (manifest, wasm) = crate::commands_addons::read_package(&zip).unwrap();
        let dir =
            std::env::temp_dir().join(format!("time-addon-test-{}-{}", manifest.id, std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(&manifest.wasm), wasm).unwrap();
        core.save_addon(&manifest.id, &manifest.version, granted)
            .await
            .unwrap();
        let record = core.set_addon_enabled(&manifest.id, true).await.unwrap();
        let notifications = Arc::new(Mutex::new(Vec::new()));
        let sink = notifications.clone();
        let services = HostServices {
            core: core.clone(),
            notify: Arc::new(move |t, b| sink.lock().unwrap().push((t.to_string(), b.to_string()))),
            entries_changed: Arc::new(|| {}),
        };
        let plugin = build_plugin(&services, &dir, &manifest, &record).unwrap();
        let addons = Addons::default();
        addons.loaded.lock().unwrap().insert(
            manifest.id.clone(),
            Arc::new(Loaded {
                manifest,
                plugin: Arc::new(Mutex::new(plugin)),
            }),
        );
        Some(Fixture {
            addons,
            core,
            notifications,
            _dir: dir,
        })
    }

    async fn project(core: &time_core::Core) -> String {
        core.create_project(ProjectInput {
            name: "Website".into(),
            alias: Some("WEB".into()),
            color: "#3b82f6".into(),
            parent_id: None,
            client: None,
            budget_minutes: None,
            budget_period: time_core::model::BudgetPeriod::Total,
            budget_warn_pct: 80,
            billable_default: false,
            status: time_core::model::ProjectStatus::Active,
            cost_center_id: None,
        })
        .await
        .unwrap()
        .id
    }

    /// A manual entry on `project` with a "Work" activity.
    async fn entry_input(core: &time_core::Core, project: &str, note: &str) -> EntryInput {
        let activity = match core.list_activities(false).await.unwrap().into_iter().next() {
            Some(a) => a,
            None => core
                .create_activity(time_core::model::ActivityInput {
                    name: "Work".into(),
                    color: "#3b82f6".into(),
                    archived: false,
                })
                .await
                .unwrap(),
        };
        EntryInput {
            activity_id: Some(activity.id),
            project_id: Some(project.into()),
            start_at: T0,
            end_at: Some(T0 + HOUR + HOUR / 2),
            tz: None,
            note: note.into(),
            billable: None,
            tag_ids: vec![],
            cost_center_id: None,
        }
    }

    #[tokio::test]
    async fn csv_export_addon_writes_the_configured_csv() {
        let Some(f) = load("dev.time.csv-export-1.0.0.zip", &[]).await else {
            return;
        };
        let pid = project(&f.core).await;
        f.core
            .create_entry(entry_input(&f.core, &pid, "Design; review").await)
            .await
            .unwrap();
        let range = Range {
            from: T0 - HOUR,
            to: T0 + 24 * HOUR,
        };
        let entries = f.core.list_entries(range).await.unwrap();
        let request = sdk::ExportRequest {
            format_id: "entries".into(),
            range: sdk::Range {
                from: range.from,
                to: range.to,
            },
            entries: sdk_entries(entries, chrono_tz::UTC),
            projects: convert(f.core.list_projects(true).await.unwrap()).unwrap(),
            regional: sdk::Regional {
                date_format: "dmy".into(),
                number_format: "comma".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let csv = f.addons.export("dev.time.csv-export", &request).await.unwrap();
        let lines: Vec<&str> = csv.trim_start_matches('\u{feff}').lines().collect();
        assert_eq!(lines[0], "date;start;end;hours;project;alias;tags;note");
        assert_eq!(
            lines[1],
            "05.10.2026;08:00;09:30;1,50;Website;WEB;;\"Design; review\""
        );
    }

    #[tokio::test]
    async fn recent_notes_addon_uses_events_storage_and_suggestions() {
        let Some(f) = load("dev.time.recent-notes-1.0.0.zip", &[Permission::ProjectsRead]).await else {
            return;
        };
        let id = "dev.time.recent-notes";
        let pid = project(&f.core).await;
        let entry = f
            .core
            .create_entry(entry_input(&f.core, &pid, "Write docs").await)
            .await
            .unwrap();

        f.addons.dispatch(&DomainEvent::EntryStopped { entry }).await;
        assert_eq!(f.addons.error(id), None);
        let stored = f.core.addon_kv_get(id, "recent").await.unwrap().unwrap();
        assert!(stored.contains("Write docs"));

        let suggestions = f.addons.suggest("docs").await;
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].1.label, "Continue: Write docs");
        assert_eq!(suggestions[0].1.detail.as_deref(), Some("Website"));
        assert_eq!(suggestions[0].1.project_id.as_deref(), Some(pid.as_str()));

        // `notify` was not granted: the call fails and nothing is shown.
        let date = "2026-10-05".parse().unwrap();
        let day = f.core.work_days(date, date).await.unwrap().remove(0);
        f.addons.dispatch(&DomainEvent::DayClosed { date, day }).await;
        let error = f.addons.error(id).unwrap();
        assert!(error.contains("permission 'notify'"), "{error}");
        assert!(f.notifications.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn granted_notify_reaches_the_user_with_the_addon_name() {
        let Some(f) = load(
            "dev.time.recent-notes-1.0.0.zip",
            &[Permission::ProjectsRead, Permission::Notify],
        )
        .await
        else {
            return;
        };
        let pid = project(&f.core).await;
        f.core
            .create_entry(entry_input(&f.core, &pid, "").await)
            .await
            .unwrap();
        let date = "2026-10-05".parse().unwrap();
        let day = f.core.work_days(date, date).await.unwrap().remove(0);
        f.addons.dispatch(&DomainEvent::DayClosed { date, day }).await;
        assert_eq!(f.addons.error("dev.time.recent-notes"), None);
        let shown = f.notifications.lock().unwrap().clone();
        assert_eq!(
            shown,
            vec![(
                "Recent notes · 2026-10-05".to_string(),
                "1:30 h worked".to_string()
            )]
        );
    }
}
