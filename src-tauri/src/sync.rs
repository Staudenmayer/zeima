//! Pushing local changes to the Zeima server (SPEC §6).
//!
//! While signed in, every minute and on demand: the outbox
//! (`Core::sync_outbox`) goes to `POST /v1/sync/push`, then
//! `GET /v1/sync/pull` brings the server's changes (`Core::sync_apply`).
//! The device syncs into the user's personal workspace on the server: the
//! first device creates it under its local id, later devices find it with
//! `GET /v1/workspaces` and use that id.

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;
use zeima_core::sync::Outcome;
use zeima_core::{RemoteRow, SYNC_BATCH};

use crate::account::{self, discover};
use crate::commands::{CmdError, CmdResult};
use crate::events::{DataChanged, DataKind, SyncChanged};
use crate::state::AppState;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// `local_meta` key: `<server>|<server workspace id>` once known.
const WORKSPACE_KEY: &str = "sync_workspace";
/// `local_meta` key: the `WORKSPACE_KEY` value this device has joined, i.e.
/// decided how its data and the server's data go together.
const JOINED_KEY: &str = "sync_joined";
/// `local_meta` key: `1` while the user has to choose (see [`JoinStrategy`]).
const JOIN_PENDING_KEY: &str = "sync_join_pending";
/// Error text of a run that waits for the user's choice.
const JOIN_NEEDED: &str = "choose how to join your account's data";
/// Batches per run, so one run cannot go on forever.
const MAX_BATCHES: usize = 20;

#[derive(Default)]
struct Last {
    at: Option<i64>,
    error: Option<String>,
    /// A run is in progress.
    syncing: bool,
}

/// What a failed sync means for the user, so the UI can say it in words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SyncErrorKind {
    /// The plan does not include syncing.
    Plan,
    /// The server could not be reached.
    Offline,
    /// The app is too old for this server.
    UpdateNeeded,
    /// The server answered with an error.
    Server,
}

fn classify(error: &str) -> SyncErrorKind {
    let e = error.to_lowercase();
    if e.contains("not part of your plan") {
        SyncErrorKind::Plan
    } else if e.contains("newer version") || e.contains("please update") || e.contains("or newer") {
        SyncErrorKind::UpdateNeeded
    } else if e.contains("could not reach")
        || e.contains("connection")
        || e.contains("timed out")
        || e.contains("dns")
        || e.contains("error sending")
    {
        SyncErrorKind::Offline
    } else {
        SyncErrorKind::Server
    }
}

fn last() -> &'static Mutex<Last> {
    static LAST: OnceLock<Mutex<Last>> = OnceLock::new();
    LAST.get_or_init(Default::default)
}

fn lock_async() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(Default::default)
}

fn get_json(url: &str, token: &str) -> Result<serde_json::Value, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(HTTP_TIMEOUT))
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("Accept", "application/json")
        .call()
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!("the server answered {status}: {text}"));
    }
    serde_json::from_str(&text).map_err(|e| format!("the server's answer: {e}"))
}

fn post_json(url: &str, token: &str, body: &serde_json::Value) -> Result<serde_json::Value, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(HTTP_TIMEOUT))
        .build()
        .into();
    let mut response = agent
        .post(url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .send(body.to_string())
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!("the server answered {status}: {text}"));
    }
    serde_json::from_str(&text).map_err(|e| format!("the server's answer: {e}"))
}

#[derive(Deserialize)]
struct PushAnswer {
    results: Vec<Outcome>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServerWorkspace {
    id: String,
    kind: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    org_id: Option<String>,
}

#[derive(Deserialize)]
struct PullAnswer {
    rows: Vec<RemoteRow>,
    cursor: i64,
    more: bool,
}

/// The server's id of this user's personal workspace, creating it (under the
/// local id) if the user has none yet. Remembered per server.
async fn server_workspace(
    app: &AppHandle,
    api: &str,
    server: &str,
    token: &str,
) -> Result<(String, bool), String> {
    let core = &app.state::<AppState>().core;
    if let Some(stored) = core.get_meta(WORKSPACE_KEY).await.map_err(|e| e.to_string())? {
        if let Some((s, id)) = stored.split_once('|') {
            if s == server {
                return Ok((id.to_string(), false));
            }
        }
    }
    let (url, tok) = (format!("{api}/v1/workspaces"), token.to_string());
    let list = tauri::async_runtime::spawn_blocking(move || get_json(&url, &tok))
        .await
        .map_err(|e| e.to_string())??;
    let existing = serde_json::from_value::<Vec<ServerWorkspace>>(list)
        .map_err(|e| format!("the server's answer: {e}"))?
        .into_iter()
        .find(|w| w.kind == "personal");
    let created = existing.is_none();
    let id = match existing {
        Some(w) => w.id,
        None => {
            let id = core.ctx().workspace_id.clone();
            let (url, tok, body) = (
                format!("{api}/v1/workspaces"),
                token.to_string(),
                serde_json::json!({ "id": id, "name": "Personal" }),
            );
            tauri::async_runtime::spawn_blocking(move || post_json(&url, &tok, &body))
                .await
                .map_err(|e| e.to_string())??;
            id
        }
    };
    // A different server has its own change feed.
    core.set_sync_cursor(0).await.map_err(|e| e.to_string())?;
    core.set_meta(WORKSPACE_KEY, &format!("{server}|{id}"))
        .await
        .map_err(|e| e.to_string())?;
    Ok((id, created))
}

/// Sends everything pending. Returns how many changes were sent.
async fn push_all(
    core: &zeima_core::Core,
    api: &str,
    token: &str,
    workspace_id: &str,
) -> Result<usize, String> {
    let device_id = core.ctx().device_id.clone();
    let mut sent = 0;
    for _ in 0..MAX_BATCHES {
        let outbox = core.sync_outbox(SYNC_BATCH).await.map_err(|e| e.to_string())?;
        if outbox.is_empty() {
            break;
        }
        let results = if outbox.changes.is_empty() {
            Vec::new()
        } else {
            let body = serde_json::json!({
                "workspaceId": workspace_id,
                "deviceId": device_id,
                "changes": outbox.changes,
            });
            let (url, tok) = (format!("{api}/v1/sync/push"), token.to_string());
            let answer = tauri::async_runtime::spawn_blocking(move || post_json(&url, &tok, &body))
                .await
                .map_err(|e| e.to_string())??;
            serde_json::from_value::<PushAnswer>(answer)
                .map_err(|e| format!("the server's answer: {e}"))?
                .results
        };
        sent += outbox.changes.len();
        core.sync_mark_pushed(&outbox, &results)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(sent)
}

/// Brings the server's changes in. Returns how many local rows changed.
async fn pull_all(
    app: &AppHandle,
    api: &str,
    token: &str,
    workspace_id: &str,
    shared: &[(String, zeima_core::Core)],
) -> Result<usize, String> {
    let core = &app.state::<AppState>().core;
    let mut applied = 0;
    for _ in 0..MAX_BATCHES {
        let cursor = core.sync_cursor().await.map_err(|e| e.to_string())?;
        let (url, tok) = (
            format!("{api}/v1/sync/pull?cursor={cursor}&limit=500"),
            token.to_string(),
        );
        let answer = tauri::async_runtime::spawn_blocking(move || get_json(&url, &tok))
            .await
            .map_err(|e| e.to_string())??;
        let page: PullAnswer =
            serde_json::from_value(answer).map_err(|e| format!("the server's answer: {e}"))?;
        // Each row goes to the workspace it belongs to; rows of workspaces
        // this device does not know are skipped.
        let (personal, others): (Vec<RemoteRow>, Vec<RemoteRow>) = page
            .rows
            .into_iter()
            .partition(|r| r.workspace_id == workspace_id);
        applied += core
            .sync_apply(&personal)
            .await
            .map_err(|e| e.to_string())?
            .applied;
        for (id, scope) in shared {
            let mine: Vec<RemoteRow> = others.iter().filter(|r| r.workspace_id == *id).cloned().collect();
            if !mine.is_empty() {
                applied += scope.sync_apply(&mine).await.map_err(|e| e.to_string())?.applied;
                // Not for the first load of a new device: that is history, not news.
                if cursor > 0 {
                    crate::approvals::show(app, &crate::approvals::notices(&mine, &scope.ctx().user_id));
                }
            }
        }
        core.set_sync_cursor(page.cursor)
            .await
            .map_err(|e| e.to_string())?;
        if !page.more {
            break;
        }
    }
    Ok(applied)
}

/// Takes the language, formats and time zone another device saved. Only a
/// difference is applied, and nothing is sent back (`set_display_settings`
/// skips equal values). Returns whether the settings changed.
async fn adopt_synced_settings(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let Ok(Some(synced)) = state.core.display_settings().await else {
        return false;
    };
    let current = state.settings();
    let merged = crate::settings::with_portable(&current, &synced);
    if merged == current {
        return false;
    }
    if crate::settings::save(&state.core, &merged).await.is_err() {
        return false;
    }
    *state.settings.write().unwrap_or_else(|e| e.into_inner()) = merged.clone();
    // A new zone or week start moves day boundaries.
    crate::settings::apply_regional(&state.core, &merged).await;
    let _ = DataChanged {
        kinds: vec![DataKind::Settings, DataKind::Entry, DataKind::Work],
    }
    .emit(app);
    true
}

/// Rows changed under the windows: have them reload.
async fn reload_windows(app: &AppHandle) {
    // The auto-pause rules are read from this copy of the work settings.
    let state = app.state::<AppState>();
    if let Ok(settings) = state.core.work_settings().await {
        state.set_work_settings(settings);
    }
    let _ = DataChanged {
        kinds: vec![
            DataKind::Activity,
            DataKind::Project,
            DataKind::Tag,
            DataKind::Entry,
            DataKind::CostCenter,
            // Schedules, absences and their types, holidays, corrections, work settings.
            DataKind::Work,
        ],
    }
    .emit(app);
    crate::tray::refresh(app).await;
}

/// Forgets a pending join choice (signing out, or another server).
pub async fn reset_join(app: &AppHandle) {
    let _ = app.state::<AppState>().core.set_meta(JOIN_PENDING_KEY, "").await;
}

/// Fetches the latest entitlement token. Offline or on a server error the
/// stored one keeps counting (its grace period applies).
async fn refresh_entitlement(app: &AppHandle, api: &str, token: &str) {
    let (url, tok) = (format!("{api}/v1/entitlement"), token.to_string());
    let Ok(Ok(answer)) = tauri::async_runtime::spawn_blocking(move || get_json(&url, &tok)).await else {
        return;
    };
    match answer.get("token").and_then(|t| t.as_str()) {
        Some(token) => {
            if let Err(err) = crate::license::store(app, token).await {
                eprintln!("{err}");
            }
        }
        // The server says Free.
        None => crate::license::clear(app).await,
    }
}

/// The organizations' shared workspaces the server lists for this account,
/// recorded on this device, each with a handle that acts in it as the
/// account (whose id is the token's subject).
async fn adopt_shared_workspaces(
    app: &AppHandle,
    api: &str,
    token: &str,
) -> Result<Vec<(String, zeima_core::Core)>, String> {
    let Some(user) = account::user_from_token(token) else {
        return Ok(Vec::new());
    };
    let (url, tok) = (format!("{api}/v1/workspaces"), token.to_string());
    let list = tauri::async_runtime::spawn_blocking(move || get_json(&url, &tok))
        .await
        .map_err(|e| e.to_string())??;
    let core = &app.state::<AppState>().core;
    // Which account the shared workspaces belong to, for switching offline.
    core.set_meta(crate::workspaces::ACCOUNT_USER_KEY, &user.id)
        .await
        .map_err(|e| e.to_string())?;
    // The departments, for the policies that apply to this person.
    let groups = serde_json::to_string(&account::groups_from_token(token)).map_err(|e| e.to_string())?;
    core.set_meta(zeima_core::ACCOUNT_GROUPS_KEY, &groups)
        .await
        .map_err(|e| e.to_string())?;
    let mut shared = Vec::new();
    for w in serde_json::from_value::<Vec<ServerWorkspace>>(list)
        .map_err(|e| format!("the server's answer: {e}"))?
        .into_iter()
        .filter(|w| w.kind == "org")
    {
        let Some(org_id) = w.org_id.as_deref() else {
            continue;
        };
        let name = user
            .name
            .clone()
            .or(user.email.clone())
            .unwrap_or_else(|| "Me".into());
        core.adopt_org_workspace(&w.id, org_id, &w.name, &user.id, &name)
            .await
            .map_err(|e| e.to_string())?;
        shared.push((w.id.clone(), core.scoped(&w.id, &user.id)));
    }
    Ok(shared)
}

/// One sync: push, then pull. Returns the number of changes sent.
async fn sync_once(app: &AppHandle) -> Result<Option<usize>, String> {
    let Some((server, token)) = account::current_token(app).await? else {
        return Ok(None);
    };
    let probe = server.clone();
    let (discovery, _) = tauri::async_runtime::spawn_blocking(move || discover(&probe))
        .await
        .map_err(|e| e.to_string())??;
    let api = discovery.api_url.trim_end_matches('/').to_string();
    let core = &app.state::<AppState>().core;
    if core.get_meta(JOIN_PENDING_KEY).await.ok().flatten().as_deref() == Some("1") {
        return Err(JOIN_NEEDED.into());
    }
    refresh_entitlement(app, &api, &token).await;
    if !crate::license::has_feature(app, "sync").await {
        return Err("syncing is not part of your plan".into());
    }
    let (workspace_id, created) = server_workspace(app, &api, &server, &token).await?;

    // The first time this device meets this server: a device that already has
    // data and finds data on the server must not guess how to put them together.
    let marker = format!("{server}|{workspace_id}");
    if core
        .get_meta(JOINED_KEY)
        .await
        .map_err(|e| e.to_string())?
        .as_deref()
        != Some(marker.as_str())
    {
        let has_local_data = !core.sync_outbox(1).await.map_err(|e| e.to_string())?.is_empty();
        if created || !has_local_data {
            core.set_meta(JOINED_KEY, &marker)
                .await
                .map_err(|e| e.to_string())?;
        } else {
            core.set_meta(JOIN_PENDING_KEY, "1")
                .await
                .map_err(|e| e.to_string())?;
            return Err(JOIN_NEEDED.into());
        }
    }

    // Organizations' shared workspaces: one handle on each, acting as the account.
    let shared = adopt_shared_workspaces(app, &api, &token).await?;
    let mut sent = push_all(core, &api, &token, &workspace_id).await?;
    for (id, scope) in &shared {
        sent += push_all(scope, &api, &token, id).await?;
    }
    let pulled = pull_all(app, &api, &token, &workspace_id, &shared).await?;
    if pulled > 0 {
        adopt_synced_settings(app).await;
        reload_windows(app).await;
    }
    Ok(Some(sent))
}

/// Runs a sync if signed in, remembering the outcome for `sync_status`.
pub async fn run(app: &AppHandle) -> Result<usize, String> {
    let _guard = lock_async().lock().await;
    last().lock().unwrap_or_else(|e| e.into_inner()).syncing = true;
    let _ = SyncChanged.emit(app);
    let result = sync_once(app).await;
    let now = app.state::<AppState>().core.now_ms();
    {
        let mut last = last().lock().unwrap_or_else(|e| e.into_inner());
        last.syncing = false;
        match &result {
            // Not signed in: nothing was synced, and nothing went wrong.
            Ok(None) => last.error = None,
            Ok(Some(_)) => {
                last.at = Some(now);
                last.error = None;
            }
            Err(e) => last.error = Some(e.clone()),
        }
    }
    let _ = SyncChanged.emit(app);
    result.map(|sent| sent.unwrap_or(0))
}

#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    /// Changes not yet sent.
    #[specta(type = specta_typescript::Number)]
    pub pending: i64,
    #[specta(type = Option<specta_typescript::Number>)]
    pub last_synced_at: Option<i64>,
    pub last_error: Option<String>,
    /// What the error means, for a message in words.
    pub last_error_kind: Option<SyncErrorKind>,
    /// A sync is running right now.
    pub syncing: bool,
    /// Server changes that could not be applied here.
    #[specta(type = specta_typescript::Number)]
    pub conflicts: i64,
    /// This device and the account both have data: the user has to choose
    /// (`sync_join`) before syncing starts.
    pub join_choice: bool,
}

#[tauri::command]
#[specta::specta]
pub async fn sync_status(app: AppHandle) -> CmdResult<SyncStatus> {
    let core = &app.state::<AppState>().core;
    let pending = core.sync_pending().await?;
    let conflicts = core.sync_conflict_count().await?;
    let join_choice = core.get_meta(JOIN_PENDING_KEY).await?.as_deref() == Some("1");
    let last = last().lock().unwrap_or_else(|e| e.into_inner());
    Ok(SyncStatus {
        pending,
        last_synced_at: last.at,
        last_error: if join_choice { None } else { last.error.clone() },
        last_error_kind: if join_choice {
            None
        } else {
            last.error.as_deref().map(classify)
        },
        syncing: last.syncing,
        conflicts,
        join_choice,
    })
}

/// A server change that could not be written on this device.
#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConflictView {
    /// `project`, `tag`, `time_entry`, …
    pub table: String,
    pub row_id: String,
    /// The server's name for the row, if it has one.
    pub name: Option<String>,
    pub message: String,
    #[specta(type = specta_typescript::Number)]
    pub at: i64,
}

#[tauri::command]
#[specta::specta]
pub async fn sync_conflicts(app: AppHandle) -> CmdResult<Vec<ConflictView>> {
    let conflicts = app.state::<AppState>().core.sync_conflicts().await?;
    Ok(conflicts
        .into_iter()
        .map(|c| ConflictView {
            table: c.table,
            row_id: c.row_id,
            name: c.name,
            message: c.message,
            at: c.at,
        })
        .collect())
}

/// One field of a conflicting row, ready to show.
#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConflictField {
    pub field: String,
    /// The account's value.
    pub server: String,
    /// This device's value; `None` when the row does not exist here.
    pub local: Option<String>,
    /// The two differ (or there is no local row).
    pub differs: bool,
}

/// A value as text: strings plain, empty as a dash, structures as compact JSON.
fn show(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "—".into(),
        serde_json::Value::String(s) if s.is_empty() => "—".into(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The account's version of a conflicting row next to this device's.
#[tauri::command]
#[specta::specta]
pub async fn sync_conflict_details(
    app: AppHandle,
    table: String,
    row_id: String,
) -> CmdResult<Vec<ConflictField>> {
    let fields = app
        .state::<AppState>()
        .core
        .sync_conflict_compare(&table, &row_id)
        .await?;
    Ok(fields
        .into_iter()
        .map(|f| {
            // SQLite keeps booleans as 0/1.
            let local = f.local.map(|l| match (&f.server, &l) {
                (serde_json::Value::Bool(_), serde_json::Value::Number(n)) => {
                    serde_json::Value::Bool(n.as_i64() == Some(1))
                }
                _ => l,
            });
            let differs = local.as_ref().is_none_or(|l| *l != f.server);
            ConflictField {
                field: f.field,
                server: show(&f.server),
                local: local.as_ref().map(show),
                differs,
            }
        })
        .collect())
}

/// Tries the server's version again; true if it was written.
#[tauri::command]
#[specta::specta]
pub async fn sync_retry_conflict(app: AppHandle, table: String, row_id: String) -> CmdResult<bool> {
    let written = app
        .state::<AppState>()
        .core
        .sync_retry_conflict(&table, &row_id)
        .await?;
    if written {
        reload_windows(&app).await;
    }
    Ok(written)
}

/// Gives up on a conflict; the local row stays as it is.
#[tauri::command]
#[specta::specta]
pub async fn sync_dismiss_conflict(app: AppHandle, table: String, row_id: String) -> CmdResult<()> {
    app.state::<AppState>()
        .core
        .sync_dismiss_conflict(&table, &row_id)
        .await?;
    Ok(())
}

/// How a device that already has data joins an account that has data too.
#[derive(Debug, Clone, Copy, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum JoinStrategy {
    /// Replace this device's data with the account's. Anything only here is lost.
    ReplaceLocal,
    /// Keep both. Items that exist on both sides (the same alias or name) are
    /// reported as conflicts to sort out.
    Merge,
}

/// Settles the choice and syncs.
#[tauri::command]
#[specta::specta]
pub async fn sync_join(app: AppHandle, strategy: JoinStrategy) -> CmdResult<SyncStatus> {
    let core = &app.state::<AppState>().core;
    if core.get_meta(JOIN_PENDING_KEY).await?.as_deref() != Some("1") {
        return sync_status(app).await;
    }
    if matches!(strategy, JoinStrategy::ReplaceLocal) {
        core.sync_wipe_local().await?;
    }
    let marker = core.get_meta(WORKSPACE_KEY).await?.unwrap_or_default();
    core.set_meta(JOINED_KEY, &marker).await?;
    core.set_meta(JOIN_PENDING_KEY, "").await?;
    if matches!(strategy, JoinStrategy::ReplaceLocal) {
        reload_windows(&app).await;
    }
    sync_now(app).await
}

/// Syncs now. Returns the status afterwards.
#[tauri::command]
#[specta::specta]
pub async fn sync_now(app: AppHandle) -> CmdResult<SyncStatus> {
    run(&app).await.map_err(|e| CmdError::other("sync", e))?;
    sync_status(app).await
}

// --------------------------------------------------------------------------
// "Changes available" channel
// --------------------------------------------------------------------------

/// `https://host` → `wss://host`.
fn websocket_url(api: &str, device_id: &str) -> String {
    let base = api.trim_end_matches('/');
    let base = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base.to_string()
    };
    format!("{base}/v1/sync/events?deviceId={device_id}")
}

/// Holds one connection open until it drops. Each "changes" message syncs
/// at once. Returns when the server closes the connection (for example when
/// the token expired) or it fails.
async fn listen_once(app: &AppHandle) -> Result<(), String> {
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::Message;

    let Some((server, token)) = account::current_token(app).await? else {
        return Err("not signed in".into());
    };
    let probe = server.clone();
    let (discovery, _) = tauri::async_runtime::spawn_blocking(move || discover(&probe))
        .await
        .map_err(|e| e.to_string())??;
    let device_id = app.state::<AppState>().core.ctx().device_id.clone();
    let mut request = websocket_url(&discovery.api_url, &device_id)
        .into_client_request()
        .map_err(|e| e.to_string())?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {token}")
            .parse()
            .map_err(|_| "invalid token".to_string())?,
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| e.to_string())?;
    while let Some(message) = socket.next().await {
        match message.map_err(|e| e.to_string())? {
            Message::Text(text) if text.as_str().contains("\"changes\"") => {
                if let Err(err) = run(app).await {
                    eprintln!("sync failed: {err}");
                }
            }
            Message::Text(text) if text.as_str().contains("\"presence\"") => {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(text.as_str()) {
                    if let Some(org_id) = value.get("orgId").and_then(|v| v.as_str()) {
                        let _ = crate::events::PresenceChanged {
                            org_id: org_id.into(),
                        }
                        .emit(app);
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    Ok(())
}

/// Keeps the "changes available" connection up while signed in, reconnecting
/// with a fresh token and a growing pause after failures. The minute timer in
/// `background.rs` still syncs if this channel is down.
pub fn spawn_listener(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut pause = Duration::from_secs(5);
        loop {
            match listen_once(&app).await {
                // Connected for a while and then closed normally: try again soon.
                Ok(()) => pause = Duration::from_secs(5),
                Err(_) => pause = (pause * 2).min(Duration::from_secs(120)),
            }
            tokio::time::sleep(pause).await;
        }
    });
}

// --------------------------------------------------------------------------
// Sending soon after a change
// --------------------------------------------------------------------------

/// How often to look for unsent changes.
const WATCH_EVERY: Duration = Duration::from_secs(3);
/// After a failed run, wait this long before trying again from here (the
/// minute timer in `background.rs` keeps trying anyway).
const RETRY_AFTER_ERROR: Duration = Duration::from_secs(30);

/// Sends changes a few seconds after they are made instead of at the next
/// minute tick. It waits until the number of unsent changes has stopped
/// growing, so a burst of edits goes out as one batch. Does nothing while
/// signed out.
pub fn spawn_push_watcher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut seen: i64 = 0;
        let mut hold_until = tokio::time::Instant::now();
        loop {
            tokio::time::sleep(WATCH_EVERY).await;
            if tokio::time::Instant::now() < hold_until {
                continue;
            }
            let core = &app.state::<AppState>().core;
            let pending = core.sync_pending().await.unwrap_or(0);
            if pending == 0 {
                seen = 0;
                continue;
            }
            // Wait for a quiet moment: the same count twice in a row.
            if pending != seen {
                seen = pending;
                continue;
            }
            seen = 0;
            if run(&app).await.is_err() {
                hold_until = tokio::time::Instant::now() + RETRY_AFTER_ERROR;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_sorted_into_what_the_user_can_do_about_them() {
        assert_eq!(classify("syncing is not part of your plan"), SyncErrorKind::Plan);
        assert_eq!(
            classify("could not reach the server: Connection Failed: tcp connect error"),
            SyncErrorKind::Offline
        );
        assert_eq!(classify("io: timed out reading response"), SyncErrorKind::Offline);
        assert_eq!(
            classify("this server needs Zeima 1.2.0 or newer; please update the app"),
            SyncErrorKind::UpdateNeeded
        );
        assert_eq!(classify("the server answered 500: oops"), SyncErrorKind::Server);
    }

    #[test]
    fn the_channel_url_follows_the_api_scheme() {
        assert_eq!(
            websocket_url("https://zeima.example.com/", "dev-1"),
            "wss://zeima.example.com/v1/sync/events?deviceId=dev-1"
        );
        assert_eq!(
            websocket_url("http://localhost:3000", "dev-1"),
            "ws://localhost:3000/v1/sync/events?deviceId=dev-1"
        );
    }
}
