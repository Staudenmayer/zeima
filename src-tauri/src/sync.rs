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
use crate::events::{DataChanged, DataKind};
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
struct ServerWorkspace {
    id: String,
    kind: String,
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
async fn push_all(app: &AppHandle, api: &str, token: &str, workspace_id: &str) -> Result<usize, String> {
    let core = &app.state::<AppState>().core;
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
async fn pull_all(app: &AppHandle, api: &str, token: &str, workspace_id: &str) -> Result<usize, String> {
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
        let rows: Vec<RemoteRow> = page
            .rows
            .into_iter()
            .filter(|r| r.workspace_id == workspace_id)
            .collect();
        let report = core.sync_apply(&rows).await.map_err(|e| e.to_string())?;
        applied += report.applied;
        core.set_sync_cursor(page.cursor)
            .await
            .map_err(|e| e.to_string())?;
        if !page.more {
            break;
        }
    }
    Ok(applied)
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

/// One sync: push, then pull. Returns the number of changes sent.
async fn sync_once(app: &AppHandle) -> Result<usize, String> {
    let Some((server, token)) = account::current_token(app).await? else {
        return Ok(0);
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

    let sent = push_all(app, &api, &token, &workspace_id).await?;
    let pulled = pull_all(app, &api, &token, &workspace_id).await?;
    if pulled > 0 {
        reload_windows(app).await;
    }
    Ok(sent)
}

/// Runs a sync if signed in, remembering the outcome for `sync_status`.
pub async fn run(app: &AppHandle) -> Result<usize, String> {
    let _guard = lock_async().lock().await;
    let result = sync_once(app).await;
    let now = app.state::<AppState>().core.now_ms();
    let mut last = last().lock().unwrap_or_else(|e| e.into_inner());
    match &result {
        Ok(_) => {
            last.at = Some(now);
            last.error = None;
        }
        Err(e) => last.error = Some(e.clone()),
    }
    result
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
    let pending = core.pending_changes().await?;
    let conflicts = core.sync_conflict_count().await?;
    let join_choice = core.get_meta(JOIN_PENDING_KEY).await?.as_deref() == Some("1");
    let last = last().lock().unwrap_or_else(|e| e.into_inner());
    Ok(SyncStatus {
        pending,
        last_synced_at: last.at,
        last_error: if join_choice { None } else { last.error.clone() },
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

#[cfg(test)]
mod tests {
    use super::*;

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
