//! IPC commands. Each one is a thin wrapper around `time-core`; after a
//! write it calls [`changed`] so windows and the tray update.

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_specta::Event;
use time_core::locale::RegionalFormats;
use time_core::model::{
    Activity as ActivityModel, ActivityInput, BulkResult, EntryInput, EntryPatch, Project, ProjectInput,
    RecentActivity, StartTimer, Tag, TagInput, TimeEntry, TimerState,
};
use time_core::summary::{Range, Summary};

use crate::activity::Activity;
use crate::events::{DataChanged, DataKind, PromptsChanged, TimerChanged};
use crate::settings::{self, AppSettings};
use crate::state::AppState;
use crate::{budget, shortcuts, tray, windows};

#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CmdError {
    /// Stable code: not_found, validation, conflict, database, …
    pub code: String,
    pub message: String,
    /// Input field a validation error refers to.
    pub field: Option<String>,
}

impl From<time_core::Error> for CmdError {
    fn from(err: time_core::Error) -> Self {
        let field = match &err {
            time_core::Error::Validation { field, .. } => Some(field.clone()),
            _ => None,
        };
        let message = match &err {
            time_core::Error::Validation { message, .. } => message.clone(),
            other => other.to_string(),
        };
        Self {
            code: err.code().into(),
            message,
            field,
        }
    }
}

impl CmdError {
    pub fn other(code: &str, message: impl ToString) -> Self {
        Self {
            code: code.into(),
            message: message.to_string(),
            field: None,
        }
    }
}

pub type CmdResult<T> = Result<T, CmdError>;

/// Tells windows what changed and refreshes the tray.
pub async fn changed(app: &AppHandle, kinds: &[DataKind]) {
    let _ = DataChanged {
        kinds: kinds.to_vec(),
    }
    .emit(app);
    if kinds.contains(&DataKind::Entry) {
        let state = app.state::<AppState>();
        if let Ok(timer) = state.core.timer_state().await {
            // Tracking started: the morning check-in is no longer needed.
            if !matches!(timer, TimerState::Idle) {
                if let Some(activity) = app.try_state::<Activity>() {
                    activity.dismiss_morning();
                }
            }
            let _ = TimerChanged(timer).emit(app);
        }
    }
    if kinds.contains(&DataKind::Entry) || kinds.contains(&DataKind::Away) {
        let _ = PromptsChanged.emit(app);
    }
    if kinds.contains(&DataKind::Entry) || kinds.contains(&DataKind::Project) {
        budget::check(app).await;
    }
    tray::refresh(app).await;
}

// --------------------------------------------------------------------------
// App
// --------------------------------------------------------------------------

#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub device_id: String,
    pub workspace_id: String,
    pub user_id: String,
    pub data_dir: String,
    /// Effective zone (settings, system, daylight saving applied).
    pub default_tz: String,
    /// The operating system's zone.
    pub system_tz: String,
    /// The operating system's locale, e.g. `de-AT`.
    pub system_locale: String,
    /// What "automatic" means on this system.
    pub system_formats: RegionalFormats,
    /// Formats in effect (settings with "automatic" resolved).
    pub formats: RegionalFormats,
    pub pending_changes: u32,
}

#[tauri::command]
#[specta::specta]
pub async fn app_info(app: AppHandle, state: State<'_, AppState>) -> CmdResult<AppInfo> {
    let ctx = state.core.ctx();
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        device_id: ctx.device_id.clone(),
        workspace_id: ctx.workspace_id.clone(),
        user_id: ctx.user_id.clone(),
        data_dir: app
            .path()
            .app_data_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        default_tz: state.core.default_tz(),
        system_tz: settings::system_timezone(),
        system_locale: settings::system_locale(),
        system_formats: settings::system_regional_formats(),
        formats: {
            let f = state.settings().formats();
            RegionalFormats {
                date: f.date,
                time: f.time,
                number: f.number,
                week_start: f.week_start,
            }
        },
        pending_changes: state.core.pending_changes().await?.try_into().unwrap_or(u32::MAX),
    })
}

// --------------------------------------------------------------------------
// Projects
// --------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn project_list(state: State<'_, AppState>, include_archived: bool) -> CmdResult<Vec<Project>> {
    Ok(state.core.list_projects(include_archived).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn project_create(
    app: AppHandle,
    state: State<'_, AppState>,
    input: ProjectInput,
) -> CmdResult<Project> {
    let project = state.core.create_project(input).await?;
    changed(&app, &[DataKind::Project]).await;
    Ok(project)
}

#[tauri::command]
#[specta::specta]
pub async fn project_update(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    input: ProjectInput,
) -> CmdResult<Project> {
    let project = state.core.update_project(&id, input).await?;
    changed(&app, &[DataKind::Project]).await;
    Ok(project)
}

#[tauri::command]
#[specta::specta]
pub async fn project_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.core.delete_project(&id).await?;
    changed(&app, &[DataKind::Project, DataKind::Tag]).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn project_recent(state: State<'_, AppState>, limit: u32) -> CmdResult<Vec<Project>> {
    Ok(state.core.recent_projects(limit).await?)
}

// --------------------------------------------------------------------------
// Activities
// --------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn activity_list(
    state: State<'_, AppState>,
    include_archived: bool,
) -> CmdResult<Vec<ActivityModel>> {
    Ok(state.core.list_activities(include_archived).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn activity_create(
    app: AppHandle,
    state: State<'_, AppState>,
    input: ActivityInput,
) -> CmdResult<ActivityModel> {
    let activity = state.core.create_activity(input).await?;
    changed(&app, &[DataKind::Activity]).await;
    Ok(activity)
}

#[tauri::command]
#[specta::specta]
pub async fn activity_update(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    input: ActivityInput,
) -> CmdResult<ActivityModel> {
    let activity = state.core.update_activity(&id, input).await?;
    changed(&app, &[DataKind::Activity]).await;
    Ok(activity)
}

#[tauri::command]
#[specta::specta]
pub async fn activity_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.core.delete_activity(&id).await?;
    changed(&app, &[DataKind::Activity]).await;
    Ok(())
}

/// Recently tracked activity/project/tag combinations, newest first.
#[tauri::command]
#[specta::specta]
pub async fn activity_recent(state: State<'_, AppState>, limit: u32) -> CmdResult<Vec<RecentActivity>> {
    Ok(state.core.recent_activities(limit).await?)
}

// --------------------------------------------------------------------------
// Tags
// --------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn tag_list(state: State<'_, AppState>) -> CmdResult<Vec<Tag>> {
    Ok(state.core.list_tags().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn tag_create(app: AppHandle, state: State<'_, AppState>, input: TagInput) -> CmdResult<Tag> {
    let tag = state.core.create_tag(input).await?;
    changed(&app, &[DataKind::Tag]).await;
    Ok(tag)
}

#[tauri::command]
#[specta::specta]
pub async fn tag_update(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    input: TagInput,
) -> CmdResult<Tag> {
    let tag = state.core.update_tag(&id, input).await?;
    changed(&app, &[DataKind::Tag]).await;
    Ok(tag)
}

#[tauri::command]
#[specta::specta]
pub async fn tag_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.core.delete_tag(&id).await?;
    changed(&app, &[DataKind::Tag, DataKind::Entry]).await;
    Ok(())
}

// --------------------------------------------------------------------------
// Entries
// --------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn entry_list(state: State<'_, AppState>, range: Range) -> CmdResult<Vec<TimeEntry>> {
    Ok(state.core.list_entries(range).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn entry_create(
    app: AppHandle,
    state: State<'_, AppState>,
    input: EntryInput,
) -> CmdResult<TimeEntry> {
    let entry = state.core.create_entry(input).await?;
    changed(&app, &[DataKind::Entry]).await;
    Ok(entry)
}

#[tauri::command]
#[specta::specta]
pub async fn entry_update(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    input: EntryInput,
) -> CmdResult<TimeEntry> {
    let entry = state.core.update_entry(&id, input).await?;
    changed(&app, &[DataKind::Entry]).await;
    Ok(entry)
}

#[tauri::command]
#[specta::specta]
pub async fn entry_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.core.delete_entry(&id).await?;
    changed(&app, &[DataKind::Entry]).await;
    Ok(())
}

/// Applies `patch` to several entries at once; locked ones are skipped.
#[tauri::command]
#[specta::specta]
pub async fn entry_bulk_update(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
    patch: EntryPatch,
) -> CmdResult<BulkResult> {
    let result = state.core.bulk_update_entries(&ids, patch).await?;
    changed(&app, &[DataKind::Entry]).await;
    Ok(result)
}

/// Deletes several entries at once; locked ones are skipped.
#[tauri::command]
#[specta::specta]
pub async fn entry_bulk_delete(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> CmdResult<BulkResult> {
    let result = state.core.bulk_delete_entries(&ids).await?;
    changed(&app, &[DataKind::Entry]).await;
    Ok(result)
}

/// `at` is unix ms. Passed as `f64` because IPC numbers are JS numbers.
#[tauri::command]
#[specta::specta]
pub async fn entry_split(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    at: f64,
) -> CmdResult<(TimeEntry, TimeEntry)> {
    let parts = state.core.split_entry(&id, at.round() as i64).await?;
    changed(&app, &[DataKind::Entry]).await;
    Ok(parts)
}

#[tauri::command]
#[specta::specta]
pub async fn entry_duplicate(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<TimeEntry> {
    let entry = state.core.duplicate_entry(&id).await?;
    changed(&app, &[DataKind::Entry]).await;
    Ok(entry)
}

#[tauri::command]
#[specta::specta]
pub async fn summaries(state: State<'_, AppState>, ranges: Vec<Range>) -> CmdResult<Vec<Summary>> {
    Ok(state.core.summaries(ranges).await?)
}

// --------------------------------------------------------------------------
// Timer
// --------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn timer_state(state: State<'_, AppState>) -> CmdResult<TimerState> {
    Ok(state.core.timer_state().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn timer_start(
    app: AppHandle,
    state: State<'_, AppState>,
    start: StartTimer,
) -> CmdResult<TimerState> {
    let timer = state.core.start_timer(start).await?;
    changed(&app, &[DataKind::Entry]).await;
    Ok(timer)
}

#[tauri::command]
#[specta::specta]
pub async fn timer_stop(app: AppHandle, state: State<'_, AppState>) -> CmdResult<TimerState> {
    let timer = state.core.stop_timer().await?;
    changed(&app, &[DataKind::Entry]).await;
    Ok(timer)
}

#[tauri::command]
#[specta::specta]
pub async fn timer_pause(app: AppHandle, state: State<'_, AppState>) -> CmdResult<TimerState> {
    let timer = state.core.pause_timer().await?;
    changed(&app, &[DataKind::Entry]).await;
    Ok(timer)
}

#[tauri::command]
#[specta::specta]
pub async fn timer_resume(app: AppHandle, state: State<'_, AppState>) -> CmdResult<TimerState> {
    let timer = state.core.resume_timer().await?;
    changed(&app, &[DataKind::Entry]).await;
    Ok(timer)
}

/// Pauses a running timer or resumes a paused one; does nothing when idle.
pub async fn toggle_pause(app: &AppHandle) -> time_core::Result<()> {
    let state = app.state::<AppState>();
    match state.core.timer_state().await? {
        TimerState::Running { .. } => {
            state.core.pause_timer().await?;
        }
        TimerState::Paused { .. } => {
            state.core.resume_timer().await?;
        }
        TimerState::Idle => return Ok(()),
    }
    changed(app, &[DataKind::Entry]).await;
    Ok(())
}

// --------------------------------------------------------------------------
// Settings & windows
// --------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn settings_get(state: State<'_, AppState>) -> CmdResult<AppSettings> {
    Ok(state.settings())
}

/// Validates and applies shortcuts and autostart before saving, so an
/// invalid shortcut never gets stored.
#[tauri::command]
#[specta::specta]
pub async fn settings_set(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: AppSettings,
) -> CmdResult<AppSettings> {
    let previous = state.settings();
    if let Some(tz) = &settings.timezone {
        time_core::model::validate_tz(tz).map_err(CmdError::from)?;
    }
    if let Err(message) = shortcuts::apply(&app, &settings) {
        let _ = shortcuts::apply(&app, &previous);
        return Err(CmdError {
            code: "validation".into(),
            message,
            field: Some("shortcuts".into()),
        });
    }
    let autolaunch = app.autolaunch();
    let result = if settings.autostart {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    };
    result.map_err(|e| CmdError::other("autostart", e))?;

    settings::save(&state.core, &settings).await?;
    *state.settings.write().unwrap_or_else(|e| e.into_inner()) = settings.clone();
    // A new zone moves day boundaries: every view reloads its data.
    if settings::apply_regional(&state.core, &settings).await {
        changed(&app, &[DataKind::Settings, DataKind::Entry, DataKind::Work]).await;
    } else {
        changed(&app, &[DataKind::Settings]).await;
    }
    Ok(settings)
}

/// IANA zone names the backend accepts, sorted.
#[tauri::command]
#[specta::specta]
pub async fn timezone_list() -> CmdResult<Vec<String>> {
    let mut zones: Vec<String> = chrono_tz::TZ_VARIANTS
        .iter()
        .map(|tz| tz.name().to_string())
        .collect();
    zones.sort();
    Ok(zones)
}

#[tauri::command]
#[specta::specta]
pub async fn window_show_main(app: AppHandle, route: Option<String>) -> CmdResult<()> {
    windows::show_main(&app, route.as_deref()).await;
    Ok(())
}

/// Opens the system print dialog for the calling window (also offers
/// "Save as PDF"). `window.print()` does not work in every webview.
#[tauri::command]
#[specta::specta]
pub async fn window_print(window: tauri::WebviewWindow) -> CmdResult<()> {
    window.print().map_err(|e| CmdError::other("print", e))
}

/// Opens the quick-switch palette (e.g. to pick an activity from a prompt).
#[tauri::command]
#[specta::specta]
pub async fn quickswitch_show(app: AppHandle) -> CmdResult<()> {
    windows::show_quickswitch(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn quickswitch_hide(app: AppHandle) -> CmdResult<()> {
    windows::hide_quickswitch(&app);
    Ok(())
}
