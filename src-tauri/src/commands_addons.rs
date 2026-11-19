//! Addon management, connected accounts, addon report exports, timeline
//! items and quick-switch suggestions.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use zeima_addon_sdk as sdk;
use zeima_core::addon::{AddonManifest, SettingType};
use zeima_core::addon_sig::SignatureCheck;
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
    /// Who made it, as far as the app can tell.
    pub signature: SignatureCheck,
    /// Why an organization the person belongs to does not let it run (and it is not running).
    pub blocked: Option<String>,
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
    /// Who made it.
    pub signature: SignatureCheck,
    /// Why this device may not install it (an organization's rules, or a signature that does not match).
    pub blocked: Option<String>,
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
    let dir = addons_dir(app).ok().map(|d| d.join(&record.id));
    let signature = dir.as_deref().map(crate::registry::installed_signature).unwrap_or_else(SignatureCheck::unsigned);
    let blocked = addons.blocked(&record.id, &signature);
    AddonInfo {
        signature,
        blocked,
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

/// What a package holds: the manifest (as written, the signature covers its bytes), the module and the
/// signature file if it has one.
pub(crate) struct Package {
    pub manifest: AddonManifest,
    pub manifest_src: Vec<u8>,
    pub wasm: Vec<u8>,
    pub signature: Option<String>,
}

/// Manifest and module bytes of a package.
pub(crate) fn read_package(path: &Path) -> Result<Package, CmdError> {
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
        // A manifest next to its module (development) is never signed.
        let manifest_src = std::fs::read(dir.join(MANIFEST_FILE)).map_err(io_err)?;
        return Ok(Package { manifest, manifest_src, wasm, signature: None });
    }

    read_zip(std::fs::File::open(path).map_err(io_err)?)
}

/// A package that is already in memory (downloaded and checked): it is parsed from those very bytes, never
/// written to a file and read back, so nothing can swap it in between.
pub(crate) fn read_package_bytes(bytes: Vec<u8>) -> Result<Package, CmdError> {
    read_zip(std::io::Cursor::new(bytes))
}

fn read_zip<R: Read + std::io::Seek>(reader: R) -> Result<Package, CmdError> {
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| invalid(format!("not a zip file: {e}")))?;
    // addon.toml at the root or in one top-level folder.
    let manifest_name = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().and_then(|f| f.name().ok().map(|n| n.to_string())))
        .filter(|n| {
            n == MANIFEST_FILE || n.matches('/').count() == 1 && n.ends_with(&format!("/{MANIFEST_FILE}"))
        })
        .min_by_key(|n| n.len())
        .ok_or_else(|| invalid(format!("{MANIFEST_FILE} not found in the package")))?;
    let prefix = manifest_name.trim_end_matches(MANIFEST_FILE).to_string();
    let mut src = String::new();
    let mut manifest_src = Vec::new();
    zip.by_name(&manifest_name)
        .map_err(io_err)?
        .take(1024 * 1024)
        .read_to_end(&mut manifest_src)
        .map_err(io_err)?;
    src.push_str(&String::from_utf8(manifest_src.clone()).map_err(|_| invalid("addon.toml is not text"))?);
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
    drop(entry);
    let mut signature = String::new();
    let signed = zip
        .by_name(&format!("{prefix}{}", zeima_core::addon_sig::SIGNATURE_FILE))
        .ok()
        .map(|f| f.take(16 * 1024).read_to_string(&mut signature).is_ok());
    Ok(Package { manifest, manifest_src, wasm, signature: signed.unwrap_or(false).then_some(signature) })
}

/// The words of the native question: who made it, what it may do, and (for an update) what is new.
fn consent_text(
    manifest: &AddonManifest,
    signature: &SignatureCheck,
    existing: Option<&AddonRecord>,
    de: bool,
) -> String {
    use zeima_core::addon::Permission as P;
    use zeima_core::addon_sig::SignatureStatus as S;
    let label = |p: &P| -> &'static str {
        match (p, de) {
            (P::EntriesRead, false) => "read your time entries",
            (P::EntriesRead, true) => "deine Zeiteinträge lesen",
            (P::EntriesWrite, false) => "create time entries",
            (P::EntriesWrite, true) => "Zeiteinträge anlegen",
            (P::ProjectsRead, false) => "read your projects",
            (P::ProjectsRead, true) => "deine Projekte lesen",
            (P::ActivitiesRead, false) => "read your activities",
            (P::ActivitiesRead, true) => "deine Tätigkeiten lesen",
            (P::TagsRead, false) => "read your tags",
            (P::TagsRead, true) => "deine Tags lesen",
            (P::CostCentersRead, false) => "read your cost centers",
            (P::CostCentersRead, true) => "deine Kostenstellen lesen",
            (P::MeetingsRead, false) => "read your meetings and answers",
            (P::MeetingsRead, true) => "deine Besprechungen und Antworten lesen",
            (P::AbsencesRead, false) => "read your absences",
            (P::AbsencesRead, true) => "deine Abwesenheiten lesen",
            (P::WorkRead, false) => "read your work days",
            (P::WorkRead, true) => "deine Arbeitstage lesen",
            (P::Notify, false) => "show notifications",
            (P::Notify, true) => "Benachrichtigungen anzeigen",
            (P::Network, false) => "connect to other computers on the network",
            (P::Network, true) => "sich mit anderen Computern im Netzwerk verbinden",
        }
    };
    let who = match (signature.status, &signature.signer) {
        (S::Trusted, Some(name)) => if de { format!("Signiert von {name}") } else { format!("Signed by {name}") },
        (S::ServerTrusted, Some(name)) => {
            if de { format!("Signiert von {name} (vom Server der Installation bestätigt)") } else { format!("Signed by {name} (vouched for by this installation's server)") }
        }
        (S::UnknownKey, _) => (if de { "Von einem unbekannten Herausgeber signiert" } else { "Signed by an unknown publisher" }).into(),
        _ => (if de { "Nicht signiert" } else { "Not signed" }).into(),
    };
    let mut hosts = manifest.allowed_hosts.clone();
    if !manifest.host_settings.is_empty() {
        hosts.push(if de { "(Adresse aus den Einstellungen)".into() } else { "(the address in its settings)".into() });
    }
    let perms: Vec<String> = manifest
        .permissions
        .iter()
        .map(|p| match p {
            P::Network if !hosts.is_empty() => format!("  • {}: {}", label(p), hosts.join(", ")),
            _ => format!("  • {}", label(p)),
        })
        .collect();
    let added: Vec<&P> = existing
        .map(|r| manifest.permissions.iter().filter(|p| !r.granted.contains(p)).collect())
        .unwrap_or_default();
    let mut text = format!("{} {} — {}\n{who}\n\n", manifest.name, manifest.version, manifest.author);
    text.push_str(if de { "Es darf:\n" } else { "It will be allowed to:\n" });
    text.push_str(&if perms.is_empty() {
        if de { "  • nichts außer dem eigenen Speicher".to_string() } else { "  • nothing beyond its own storage".to_string() }
    } else {
        perms.join("\n")
    });
    if existing.is_some() && !added.is_empty() {
        text.push_str(if de { "\n\nNEU in dieser Version:\n" } else { "\n\nNEW in this version:\n" });
        text.push_str(&added.iter().map(|p| format!("  • {}", label(p))).collect::<Vec<_>>().join("\n"));
    }
    text
}

/// The native yes/no. `false` for no (or if the window cannot be shown).
async fn ask_consent(app: &AppHandle, text: String, de: bool) -> bool {
    use tauri_plugin_dialog::{MessageDialogButtons, MessageDialogKind};
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(text)
        .title(if de { "Addon installieren?" } else { "Install addon?" })
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            (if de { "Installieren" } else { "Install" }).into(),
            (if de { "Abbrechen" } else { "Cancel" }).into(),
        ))
        .show(move |yes| {
            let _ = tx.send(yes);
        });
    rx.await.unwrap_or(false)
}

fn manifest_toml(manifest: &AddonManifest) -> Result<String, CmdError> {
    toml::to_string_pretty(manifest).map_err(|e| invalid(e.to_string()))
}

/// Lets the user pick a package and shows what it asks for.
#[tauri::command]
#[specta::specta]
pub async fn addon_pick(app: AppHandle, state: State<'_, AppState>, addons: State<'_, Addons>) -> CmdResult<Option<AddonPackage>> {
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
    let package = read_package(&path)?;
    let signature = addons.signature_of(&package);
    let blocked = addons.install_blocked(&package.manifest.id, &signature);
    let manifest = package.manifest;
    let installed_version = state.core.get_addon(&manifest.id).await.ok().map(|r| r.version);
    Ok(Some(AddonPackage {
        path: path.display().to_string(),
        compatible: manifest.supports_app(&app.package_info().version.to_string()),
        installed_version,
        signature,
        blocked,
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
    let package = read_package(&PathBuf::from(&path))?;
    install_package(&app, &state, &addons, package).await
}

/// Checks a package against what this device allows and installs it (enabled, all requested
/// permissions granted).
pub(crate) async fn install_package(
    app: &AppHandle,
    state: &AppState,
    addons: &Addons,
    package: Package,
) -> CmdResult<AddonInfo> {
    let signature = addons.signature_of(&package);
    if let Some(reason) = addons.install_blocked(&package.manifest.id, &signature) {
        return Err(invalid(reason));
    }
    let Package { manifest, wasm, .. } = package;
    if !manifest.supports_app(&app.package_info().version.to_string()) {
        return Err(invalid(format!(
            "needs app version {} or newer",
            manifest.min_app_version.clone().unwrap_or_default()
        )));
    }
    // Consent is asked here, by the app itself, and not only in the web view: what runs on the person's machine
    // must not depend on a front end that could be tampered with (or a call that skips it). Everything the
    // manifest asks for is granted by a yes; an update lists what is new.
    let existing = state.core.get_addon(&manifest.id).await.ok();
    let language = state.settings().language();
    let question = consent_text(&manifest, &signature, existing.as_ref(), language == "de");
    if !ask_consent(app, question, language == "de").await {
        return Err(CmdError::other("cancelled", "not installed"));
    }
    let root = addons_dir(app).map_err(io_err)?;
    let target = root.join(&manifest.id);
    let staging = root.join(format!("{}.installing", manifest.id));
    let _ = std::fs::remove_dir_all(&staging);
    let wasm_path = staging.join(&manifest.wasm);
    std::fs::create_dir_all(wasm_path.parent().unwrap_or(&staging)).map_err(io_err)?;
    std::fs::write(&wasm_path, &wasm).map_err(io_err)?;
    std::fs::write(staging.join(MANIFEST_FILE), manifest_toml(&manifest)?).map_err(io_err)?;
    crate::registry::write_signature(&staging, &signature).map_err(io_err)?;
    addons.unload(&manifest.id);
    let _ = std::fs::remove_dir_all(&target);
    std::fs::rename(&staging, &target).map_err(io_err)?;

    state
        .core
        .save_addon(&manifest.id, &manifest.version, &manifest.permissions)
        .await?;
    let record = state.core.set_addon_enabled(&manifest.id, true).await?;
    addons.reload(app, &record);
    Ok(info(app, addons, record).await)
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
            crate::trust::open_link(&opener, url)
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
    pub billable: bool,
    pub cost_center_id: Option<String>,
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
            billable: false,
            cost_center_id: None,
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
                activity_id: o.meeting.activity_id,
                project_id: o.meeting.project_id,
                tag_ids: o.meeting.tag_ids,
                billable: o.meeting.billable,
                cost_center_id: o.meeting.cost_center_id,
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

#[cfg(test)]
mod tests {
    use std::io::Write;

    use base64::Engine;
    use zeima_core::addon_sig::{check, parse_public_key, SignatureStatus, TrustedKey};

    use super::*;

    fn manifest_with(permissions: &str) -> AddonManifest {
        AddonManifest::parse(&format!(
            "id = \"dev.example.demo\"\nname = \"Demo\"\nversion = \"2.0.0\"\nauthor = \"Example\"\nwasm = \"demo.wasm\"\npermissions = [{permissions}]\n{}",
            if permissions.contains("network") { "allowed_hosts = [\"api.example.com\"]\n" } else { "" }
        ))
        .unwrap()
    }

    #[test]
    fn the_question_says_who_made_it_what_it_may_do_and_what_an_update_adds() {
        use zeima_core::addon::Permission;
        use zeima_core::addon_sig::SignatureStatus;
        let signature = SignatureCheck { status: SignatureStatus::ServerTrusted, key_id: Some("k".into()), signer: Some("Acme IT".into()) };
        let manifest = manifest_with("\"entries:read\", \"network\"");
        let fresh = consent_text(&manifest, &signature, None, false);
        assert!(fresh.contains("Demo 2.0.0 — Example"));
        assert!(fresh.contains("Signed by Acme IT (vouched for by this installation's server)"));
        assert!(fresh.contains("read your time entries"));
        assert!(fresh.contains("connect to other computers on the network: api.example.com"));
        assert!(!fresh.contains("NEW"));

        // An update that adds the network permission says so; one that adds nothing does not.
        let record = |granted: Vec<Permission>| AddonRecord {
            id: "dev.example.demo".into(),
            version: "1.0.0".into(),
            enabled: true,
            granted,
            settings: Default::default(),
            installed_at: 0,
            updated_at: 0,
        };
        let update = consent_text(&manifest, &signature, Some(&record(vec![Permission::EntriesRead])), false);
        assert!(update.contains("NEW in this version:\n  • connect to other computers"));
        let same = consent_text(&manifest, &signature, Some(&record(vec![Permission::EntriesRead, Permission::Network])), false);
        assert!(!same.contains("NEW"));
        // Unsigned and in German.
        let unsigned = consent_text(&manifest_with(""), &SignatureCheck::unsigned(), None, true);
        assert!(unsigned.contains("Nicht signiert") && unsigned.contains("nichts außer dem eigenen Speicher"));
    }

    /// A signed package built the way `scripts/addons.mjs` makes it is read, and its signature checks out;
    /// changing the module afterwards does not.
    #[test]
    fn a_signed_package_is_read_and_checked() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../../packages/shared-fixtures/addons/vectors.json")).unwrap();
        let good = &v["cases"][0];
        let manifest = good["manifest"].as_str().unwrap();
        let wasm = base64::engine::general_purpose::STANDARD.decode(good["wasmBase64"].as_str().unwrap()).unwrap();
        let signature = good["signature"].to_string();
        let trusted = [TrustedKey {
            key_id: v["key"]["keyId"].as_str().unwrap().into(),
            name: "Test".into(),
            key: parse_public_key(v["key"]["publicKey"].as_str().unwrap()).unwrap(),
            vendor: true,
        }];

        let write = |name: &str, wasm: &[u8], sig: Option<&str>| {
            let path = std::env::temp_dir().join(format!("zeima-pkg-{}-{name}.zip", std::process::id()));
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("addon.toml", opts).unwrap();
            zip.write_all(manifest.as_bytes()).unwrap();
            zip.start_file("demo.wasm", opts).unwrap();
            zip.write_all(wasm).unwrap();
            if let Some(sig) = sig {
                zip.start_file("addon.sig", opts).unwrap();
                zip.write_all(sig.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
            path
        };

        let path = write("good", &wasm, Some(&signature));
        let p = read_package(&path).unwrap();
        assert_eq!(p.manifest.id, "dev.example.demo");
        assert_eq!(check(&p.manifest_src, &p.wasm, p.signature.as_deref(), &trusted).status, SignatureStatus::Trusted);

        let mut changed = wasm.clone();
        changed.push(1);
        let path2 = write("changed", &changed, Some(&signature));
        let p = read_package(&path2).unwrap();
        assert_eq!(check(&p.manifest_src, &p.wasm, p.signature.as_deref(), &trusted).status, SignatureStatus::Invalid);

        let path3 = write("unsigned", &wasm, None);
        let p = read_package(&path3).unwrap();
        assert!(p.signature.is_none());
        assert_eq!(check(&p.manifest_src, &p.wasm, p.signature.as_deref(), &trusted).status, SignatureStatus::Unsigned);
        for f in [path, path2, path3] {
            let _ = std::fs::remove_file(f);
        }
    }
}
