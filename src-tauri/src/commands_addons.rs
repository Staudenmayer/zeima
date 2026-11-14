//! Addon management, connected accounts, addon report exports, timeline
//! items and quick-switch suggestions.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use zeima_addon_sdk as sdk;
use zeima_core::addon::{AddonManifest, SettingType};
use zeima_core::report::ReportQuery;
use zeima_core::AddonRecord;

use crate::addons::{addons_dir, read_manifest, Addons, HostServices, MANIFEST_FILE};
use crate::commands::{CmdError, CmdResult};
use crate::commands_report::{ask_save_path, write_file};
use crate::oauth;
use crate::secrets::{entry_name, Keychain, SecretStore};
use crate::state::AppState;

/// Largest accepted WASM module.
const MAX_WASM_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AddonInfo {
    pub record: AddonRecord,
    /// `None` if the installed files are missing or broken (see `error`).
    pub manifest: Option<AddonManifest>,
    pub loaded: bool,
    pub error: Option<String>,
    /// `[[oauth]]` providers with a connected account.
    pub connected: Vec<String>,
}

/// A package the user picked, before installing.
#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AddonPackage {
    pub path: String,
    pub manifest: AddonManifest,
    /// Version already installed, if this is an update.
    pub installed_version: Option<String>,
    /// The app is new enough (`min_app_version`).
    pub compatible: bool,
}

fn io_err(e: impl ToString) -> CmdError {
    CmdError::other("io", e)
}

fn invalid(message: impl ToString) -> CmdError {
    CmdError {
        code: "validation".into(),
        message: message.to_string(),
        field: Some("addon".into()),
    }
}

async fn info(app: &AppHandle, addons: &Addons, record: AddonRecord) -> AddonInfo {
    let manifest = addons_dir(app)
        .ok()
        .and_then(|d| read_manifest(&d.join(&record.id)).ok());
    let core = &app.state::<AppState>().core;
    let connected = core
        .addon_secret_names(&record.id)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter_map(|n| n.strip_prefix("oauth:").map(str::to_string))
        .collect();
    AddonInfo {
        connected,
        loaded: addons.is_loaded(&record.id),
        error: addons.error(&record.id),
        manifest,
        record,
    }
}

// --------------------------------------------------------------------------
// Packages: a .zip with addon.toml and the module, or an addon.toml next
// to its module (handy while developing).
// --------------------------------------------------------------------------

/// Manifest and module bytes of a package.
pub(crate) fn read_package(path: &Path) -> Result<(AddonManifest, Vec<u8>), CmdError> {
    if path.file_name().is_some_and(|n| n == MANIFEST_FILE) {
        let dir = path.parent().ok_or_else(|| invalid("no folder"))?;
        let manifest = read_manifest(dir).map_err(invalid)?;
        let wasm_path = dir.join(&manifest.wasm);
        let size = std::fs::metadata(&wasm_path)
            .map_err(|e| invalid(format!("{}: {e}", manifest.wasm)))?
            .len();
        if size > MAX_WASM_BYTES {
            return Err(invalid("the module is larger than 50 MB"));
        }
        let wasm = std::fs::read(&wasm_path).map_err(io_err)?;
        return Ok((manifest, wasm));
    }

    let file = std::fs::File::open(path).map_err(io_err)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| invalid(format!("not a zip file: {e}")))?;
    // addon.toml at the root or in one top-level folder.
    let manifest_name = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.name().to_string()))
        .filter(|n| {
            n == MANIFEST_FILE || n.matches('/').count() == 1 && n.ends_with(&format!("/{MANIFEST_FILE}"))
        })
        .min_by_key(|n| n.len())
        .ok_or_else(|| invalid(format!("{MANIFEST_FILE} not found in the package")))?;
    let prefix = manifest_name.trim_end_matches(MANIFEST_FILE).to_string();
    let mut src = String::new();
    zip.by_name(&manifest_name)
        .map_err(io_err)?
        .take(1024 * 1024)
        .read_to_string(&mut src)
        .map_err(io_err)?;
    let manifest = AddonManifest::parse(&src).map_err(|e| invalid(e.to_string()))?;
    // Only the two named files are read, so nothing else in the archive
    // (paths like ../x) is ever written to disk.
    let mut entry = zip
        .by_name(&format!("{prefix}{}", manifest.wasm))
        .map_err(|_| invalid(format!("{} not found in the package", manifest.wasm)))?;
    if entry.size() > MAX_WASM_BYTES {
        return Err(invalid("the module is larger than 50 MB"));
    }
    let mut wasm = Vec::new();
    entry
        .by_ref()
        .take(MAX_WASM_BYTES + 1)
        .read_to_end(&mut wasm)
        .map_err(io_err)?;
    if !wasm.starts_with(b"\0asm") {
        return Err(invalid(format!("{} is not a WASM module", manifest.wasm)));
    }
    Ok((manifest, wasm))
}

fn manifest_toml(manifest: &AddonManifest) -> Result<String, CmdError> {
    toml::to_string_pretty(manifest).map_err(|e| invalid(e.to_string()))
}

/// Lets the user pick a package and shows what it asks for.
#[tauri::command]
#[specta::specta]
pub async fn addon_pick(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Option<AddonPackage>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("Zeima addon", &["zip", "zeimaaddon", "toml"])
        .pick_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(path) = rx.await.ok().flatten().and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let (manifest, _) = read_package(&path)?;
    let installed_version = state.core.get_addon(&manifest.id).await.ok().map(|r| r.version);
    Ok(Some(AddonPackage {
        path: path.display().to_string(),
        compatible: manifest.supports_app(&app.package_info().version.to_string()),
        installed_version,
        manifest,
    }))
}

/// Installs (or updates) a package the user has consented to; it is
/// enabled right away with all requested permissions granted.
#[tauri::command]
#[specta::specta]
pub async fn addon_install(
    app: AppHandle,
    state: State<'_, AppState>,
    addons: State<'_, Addons>,
    path: String,
) -> CmdResult<AddonInfo> {
    let (manifest, wasm) = read_package(&PathBuf::from(&path))?;
    if !manifest.supports_app(&app.package_info().version.to_string()) {
        return Err(invalid(format!(
            "needs app version {} or newer",
            manifest.min_app_version.clone().unwrap_or_default()
        )));
    }
    let root = addons_dir(&app).map_err(io_err)?;
    let target = root.join(&manifest.id);
    let staging = root.join(format!("{}.installing", manifest.id));
    let _ = std::fs::remove_dir_all(&staging);
    let wasm_path = staging.join(&manifest.wasm);
    std::fs::create_dir_all(wasm_path.parent().unwrap_or(&staging)).map_err(io_err)?;
    std::fs::write(&wasm_path, &wasm).map_err(io_err)?;
    std::fs::write(staging.join(MANIFEST_FILE), manifest_toml(&manifest)?).map_err(io_err)?;
    addons.unload(&manifest.id);
    let _ = std::fs::remove_dir_all(&target);
    std::fs::rename(&staging, &target).map_err(io_err)?;

    state
        .core
        .save_addon(&manifest.id, &manifest.version, &manifest.permissions)
        .await?;
    let record = state.core.set_addon_enabled(&manifest.id, true).await?;
    addons.reload(&app, &record);
    Ok(info(&app, &addons, record).await)
}

#[tauri::command]
#[specta::specta]
pub async fn addon_list(
    app: AppHandle,
    state: State<'_, AppState>,
    addons: State<'_, Addons>,
) -> CmdResult<Vec<AddonInfo>> {
    let mut out = Vec::new();
    for record in state.core.list_addons().await? {
        out.push(info(&app, &addons, record).await);
    }
    Ok(out)
}

#[tauri::command]
#[specta::specta]
pub async fn addon_set_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    addons: State<'_, Addons>,
    id: String,
    enabled: bool,
) -> CmdResult<AddonInfo> {
    let record = state.core.set_addon_enabled(&id, enabled).await?;
    addons.reload(&app, &record);
    Ok(info(&app, &addons, record).await)
}

/// Saves the settings form; the addon is reloaded with the new values.
#[tauri::command]
#[specta::specta]
pub async fn addon_save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    addons: State<'_, Addons>,
    id: String,
    settings: BTreeMap<String, String>,
) -> CmdResult<AddonInfo> {
    let dir = addons_dir(&app).map_err(io_err)?.join(&id);
    let manifest = read_manifest(&dir).map_err(invalid)?;
    let mut clean = BTreeMap::new();
    for field in &manifest.settings {
        let value = settings
            .get(&field.key)
            .map(|v| v.trim().to_string())
            .unwrap_or_default();
        let fail = |message: &str| CmdError {
            code: "validation".into(),
            message: message.into(),
            field: Some(field.key.clone()),
        };
        if value.is_empty() {
            if field.required {
                return Err(fail("is required"));
            }
            continue;
        }
        match field.kind {
            SettingType::Number if value.replace(',', ".").parse::<f64>().is_err() => {
                return Err(fail("must be a number"))
            }
            SettingType::Boolean if value != "true" && value != "false" => {
                return Err(fail("must be true or false"))
            }
            SettingType::Select if !field.options.iter().any(|o| o.value == value) => {
                return Err(fail("is not one of the options"))
            }
            _ => {}
        }
        clean.insert(field.key.clone(), value);
    }
    let record = state.core.set_addon_settings(&id, &clean).await?;
    addons.reload(&app, &record);
    Ok(info(&app, &addons, record).await)
}

/// Removes the addon, its files, its storage and its keychain entries.
#[tauri::command]
#[specta::specta]
pub async fn addon_uninstall(
    app: AppHandle,
    state: State<'_, AppState>,
    addons: State<'_, Addons>,
    id: String,
) -> CmdResult<()> {
    addons.unload(&id);
    addons.cancel_sign_in(&id);
    addons.tokens.forget(&id, None);
    let entries: Vec<String> = state
        .core
        .addon_secret_names(&id)
        .await?
        .iter()
        .map(|name| entry_name(&id, name))
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        for entry in entries {
            if let Err(e) = Keychain.delete(&entry) {
                eprintln!("{e}");
            }
        }
    })
    .await
    .map_err(io_err)?;
    state.core.delete_addon(&id).await?;
    // The id is validated at install, but never let it escape the folder.
    if !id.is_empty() && !id.contains(['/', '\\']) && id != "." && id != ".." {
        let _ = std::fs::remove_dir_all(addons_dir(&app).map_err(io_err)?.join(&id));
    }
    Ok(())
}

// --------------------------------------------------------------------------
// Connected accounts (`[[oauth]]`)
// --------------------------------------------------------------------------

fn find_provider(
    addons: &Addons,
    id: &str,
    provider_id: &str,
) -> Result<zeima_core::addon::OAuthProvider, CmdError> {
    addons
        .manifest(id)
        .ok_or_else(|| invalid("the addon is not enabled"))?
        .oauth
        .into_iter()
        .find(|p| p.id == provider_id)
        .ok_or_else(|| invalid(format!("unknown account type '{provider_id}'")))
}

/// What the browser shows after the redirect.
pub(crate) fn sign_in_pages(language: &str) -> oauth::Pages {
    let (title, ok, failed) = if language == "de" {
        (
            "Zeima",
            "Angemeldet. Du kannst dieses Fenster schließen.",
            "Die Anmeldung ist fehlgeschlagen. Du kannst dieses Fenster schließen.",
        )
    } else {
        (
            "Zeima",
            "Signed in. You can close this window.",
            "The sign-in failed. You can close this window.",
        )
    };
    let page = |text: &str| {
        format!(
            "<!doctype html><html lang=\"{language}\"><meta charset=\"utf-8\"><title>{title}</title>\
             <body style=\"font-family:system-ui,sans-serif;display:grid;place-items:center;height:90vh;margin:0\">\
             <p>{text}</p></body></html>"
        )
    };
    oauth::Pages {
        success: page(ok),
        failure: page(failed),
    }
}

/// Signs in to an addon's account in the system browser. Resolves when the
/// sign-in is done, failed, timed out (5 minutes) or was cancelled (error
/// code `cancelled`).
#[tauri::command]
#[specta::specta]
pub async fn addon_oauth_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    addons: State<'_, Addons>,
    id: String,
    provider_id: String,
) -> CmdResult<AddonInfo> {
    let provider = find_provider(&addons, &id, &provider_id)?;
    let services = HostServices::for_app(&app);
    let cancel = addons.begin_sign_in(&id);
    let pages = sign_in_pages(state.settings().language());
    let opener = app.clone();
    let result = oauth::connect(
        &id,
        &provider,
        services.secrets.clone(),
        services.post_form.clone(),
        &services.tokens,
        state.core.now_ms(),
        move |url| {
            opener
                .opener()
                .open_url(url, None::<&str>)
                .map_err(|e| format!("could not open the browser: {e}"))
        },
        cancel,
        &pages,
    )
    .await;
    addons.end_sign_in(&id);
    match result {
        Ok(()) => {}
        Err(e) if e == "cancelled" => return Err(CmdError::other("cancelled", e)),
        Err(e) => return Err(CmdError::other("oauth", format!("{}: {e}", provider.name))),
    }
    state
        .core
        .set_addon_secret_name(&id, &oauth::secret_name(&provider.id), true)
        .await?;
    // Sync right away instead of at the next interval.
    addons.tick_soon(&id);
    let record = state.core.get_addon(&id).await?;
    Ok(info(&app, &addons, record).await)
}

/// Stops waiting for a sign-in started with `addon_oauth_connect`.
#[tauri::command]
#[specta::specta]
pub async fn addon_oauth_cancel(addons: State<'_, Addons>, id: String) -> CmdResult<()> {
    addons.cancel_sign_in(&id);
    Ok(())
}

/// Forgets a connected account (the provider may still list the app as
/// authorized until the user removes it there).
#[tauri::command]
#[specta::specta]
pub async fn addon_oauth_disconnect(
    app: AppHandle,
    state: State<'_, AppState>,
    addons: State<'_, Addons>,
    id: String,
    provider_id: String,
) -> CmdResult<AddonInfo> {
    let name = oauth::secret_name(&provider_id);
    let entry = entry_name(&id, &name);
    tauri::async_runtime::spawn_blocking(move || Keychain.delete(&entry))
        .await
        .map_err(io_err)?
        .map_err(|e| CmdError::other("keychain", e))?;
    addons.tokens.forget(&id, Some(&provider_id));
    state.core.set_addon_secret_name(&id, &name, false).await?;
    let record = state.core.get_addon(&id).await?;
    crate::background::timeline_changed(&app);
    Ok(info(&app, &addons, record).await)
}

// --------------------------------------------------------------------------
// Timeline
// --------------------------------------------------------------------------

/// An addon item in the timesheet calendar (`timeline.events`).
#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AddonTimelineItem {
    pub addon_id: String,
    pub id: String,
    pub title: String,
    #[specta(type = specta_typescript::Number)]
    pub start_at: i64,
    #[specta(type = specta_typescript::Number)]
    pub end_at: i64,
    pub all_day: bool,
    pub detail: Option<String>,
    pub color: Option<String>,
    pub activity_id: Option<String>,
    pub project_id: Option<String>,
    pub tag_ids: Vec<String>,
    pub note: String,
}

/// Items of all addons overlapping `[from, to)` (unix ms; `f64` because
/// IPC numbers are JS numbers).
#[tauri::command]
#[specta::specta]
pub async fn addon_timeline(
    state: State<'_, AppState>,
    addons: State<'_, Addons>,
    from: f64,
    to: f64,
) -> CmdResult<Vec<AddonTimelineItem>> {
    let request = sdk::TimelineRequest {
        range: sdk::Range {
            from: from.round() as i64,
            to: to.round() as i64,
        },
        timezone: state.core.default_tz(),
    };
    let mut items: Vec<_> = addons
        .timeline(&request)
        .await
        .into_iter()
        .map(|(addon_id, e)| AddonTimelineItem {
            addon_id,
            note: e.note.unwrap_or_else(|| e.title.clone()),
            id: e.id,
            title: e.title,
            start_at: e.start_at,
            end_at: e.end_at,
            all_day: e.all_day,
            detail: e.detail,
            color: e.color.filter(|c| is_hex_color(c)),
            activity_id: e.activity_id,
            project_id: e.project_id,
            tag_ids: e.tag_ids,
        })
        .collect();
    // The meetings of an organization's workspace are on the timeline too, so
    // "track this event" works for them (those declined or called off are left out).
    let core = state.data();
    if core.ctx().shared {
        let (from, to) = (from.round() as i64, to.round() as i64);
        for o in core.meetings_between(from, to).await? {
            if o.meeting.cancelled || o.my_status == Some(zeima_core::RsvpStatus::Declined) {
                continue;
            }
            items.push(AddonTimelineItem {
                addon_id: "zeima.meetings".into(),
                id: format!("{}@{}", o.meeting.id, o.start_at),
                note: o.meeting.title.clone(),
                title: o.meeting.title,
                start_at: o.start_at,
                end_at: o.end_at,
                all_day: false,
                detail: o.meeting.location,
                color: Some("#3b82f6".into()),
                activity_id: None,
                project_id: o.meeting.project_id,
                tag_ids: Vec::new(),
            });
        }
    }
    items.sort_by_key(|i| (i.start_at, i.end_at));
    Ok(items)
}

/// `#rgb` or `#rrggbb`; anything else could inject CSS.
fn is_hex_color(c: &str) -> bool {
    c.strip_prefix('#')
        .is_some_and(|h| matches!(h.len(), 3 | 6) && h.chars().all(|c| c.is_ascii_hexdigit()))
}

// --------------------------------------------------------------------------
// Report export formats
// --------------------------------------------------------------------------

#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AddonExportFormat {
    pub addon_id: String,
    pub addon_name: String,
    pub format_id: String,
    pub name: String,
    pub extension: String,
}

#[tauri::command]
#[specta::specta]
pub async fn addon_export_formats(addons: State<'_, Addons>) -> CmdResult<Vec<AddonExportFormat>> {
    Ok(addons
        .export_formats()
        .into_iter()
        .map(|(addon_id, addon_name, f)| AddonExportFormat {
            addon_id,
            addon_name,
            format_id: f.id,
            name: f.name,
            extension: f.extension,
        })
        .collect())
}

fn to_json_value<T: Serialize>(v: T) -> serde_json::Value {
    serde_json::to_value(v).unwrap_or_default()
}

/// Core value → SDK type with the same JSON shape.
fn convert_json<T: serde::de::DeserializeOwned + Default>(v: serde_json::Value) -> T {
    serde_json::from_value(v).unwrap_or_default()
}

fn enum_name<T: Serialize>(v: T) -> String {
    to_json_value(v).as_str().unwrap_or_default().to_string()
}

/// Exports the report with an addon's format to a file the user picks.
#[tauri::command]
#[specta::specta]
pub async fn report_export_addon(
    app: AppHandle,
    state: State<'_, AppState>,
    addons: State<'_, Addons>,
    query: ReportQuery,
    addon_id: String,
    format_id: String,
    file_stem: String,
) -> CmdResult<Option<String>> {
    let format = addons
        .export_formats()
        .into_iter()
        .find(|(a, _, f)| *a == addon_id && f.id == format_id)
        .ok_or_else(|| invalid("unknown export format"))?
        .2;
    let core = &state.core;
    let settings = state.settings();
    let formats = settings.formats();
    let report = core.report(&query).await?;
    let request = sdk::ExportRequest {
        format_id: format.id.clone(),
        range: sdk::Range {
            from: query.range.from,
            to: query.range.to,
        },
        group_by: query.group_by.iter().map(enum_name).collect(),
        total_ms: report.total_ms,
        billable_ms: report.billable_ms,
        rows: convert_json(to_json_value(&report.rows)),
        entries: crate::addons::sdk_entries(report.entries.clone(), core.tz()?),
        projects: convert_json(to_json_value(core.list_projects(true).await?)),
        tags: convert_json(to_json_value(core.list_tags().await?)),
        cost_centers: convert_json(to_json_value(core.list_cost_centers().await?)),
        regional: sdk::Regional {
            language: settings.language().into(),
            timezone: core.default_tz(),
            date_format: enum_name(formats.date),
            time_format: enum_name(formats.time),
            number_format: enum_name(formats.number),
            week_start: enum_name(formats.week_start),
        },
    };
    let content = addons
        .export(&addon_id, &request)
        .await
        .map_err(|e| CmdError::other("addon", e))?;
    let name = format!("{file_stem}.{}", format.extension);
    let Some(path) = ask_save_path(&app, &name, (&format.name, &format.extension)).await else {
        return Ok(None);
    };
    write_file(&path, content.as_bytes())?;
    Ok(Some(path.display().to_string()))
}

// --------------------------------------------------------------------------
// Quick switch
// --------------------------------------------------------------------------

#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AddonSuggestion {
    pub addon_id: String,
    pub label: String,
    pub detail: Option<String>,
    pub activity_id: Option<String>,
    pub project_id: Option<String>,
    pub tag_ids: Vec<String>,
    pub note: String,
}

#[tauri::command]
#[specta::specta]
pub async fn addon_suggest(addons: State<'_, Addons>, query: String) -> CmdResult<Vec<AddonSuggestion>> {
    Ok(addons
        .suggest(&query)
        .await
        .into_iter()
        .map(|(addon_id, s)| AddonSuggestion {
            addon_id,
            label: s.label,
            detail: s.detail,
            activity_id: s.activity_id,
            project_id: s.project_id,
            tag_ids: s.tag_ids,
            note: s.note,
        })
        .collect())
}
