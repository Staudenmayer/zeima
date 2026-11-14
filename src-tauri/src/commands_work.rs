//! IPC commands for work time (country profile, schedules, absences,
//! holidays, balance) and auto-pause prompts.

use chrono::NaiveDate;
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, State};
use zeima_core::jurisdiction::{self, ProfileInfo};
use zeima_core::model::{
    Absence, AbsenceInput, AbsenceType, AbsenceTypeInput, BalanceAdjustment, BalanceAdjustmentInput,
    CustomHoliday, JurisdictionSelection, WorkSettings,
};
use zeima_core::worktime::{WorkDay, WorkSchedule};
use zeima_core::{
    AwayItem, AwayResolution, Balance, HolidayView, ProfileSummary, TimesheetSubmission, VacationSummary,
    WorkScheduleInput,
};

use crate::activity::Activity;
use crate::commands::{changed, CmdError};
use crate::events::DataKind;
use crate::state::AppState;
use crate::windows;

type CmdResult<T> = Result<T, CmdError>;

async fn work_changed(app: &AppHandle) {
    changed(app, &[DataKind::Work]).await;
}

// --------------------------------------------------------------------------
// Country profile
// --------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn profile_list() -> Vec<ProfileInfo> {
    jurisdiction::list()
}

/// Profile suggested from the OS locale (e.g. `de-AT` → `AT`).
#[tauri::command]
#[specta::specta]
pub fn profile_suggest() -> Option<String> {
    sys_locale::get_locale().and_then(|l| jurisdiction::suggest_for_locale(&l))
}

#[tauri::command]
#[specta::specta]
pub async fn profile_summary(state: State<'_, AppState>, lang: String) -> CmdResult<ProfileSummary> {
    Ok(state.data().profile_summary(&lang).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn jurisdiction_get(state: State<'_, AppState>) -> CmdResult<Option<JurisdictionSelection>> {
    Ok(state.data().jurisdiction().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn jurisdiction_set(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
    region: Option<String>,
) -> CmdResult<()> {
    state
        .data()
        .set_jurisdiction(&profile_id, region.as_deref())
        .await?;
    work_changed(&app).await;
    Ok(())
}

// --------------------------------------------------------------------------
// Work settings and schedules
// --------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn work_settings_get(state: State<'_, AppState>) -> CmdResult<WorkSettings> {
    Ok(state.data().work_settings().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn work_settings_set(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: WorkSettings,
) -> CmdResult<WorkSettings> {
    let saved = state.data().set_work_settings(settings).await?;
    state.set_work_settings(saved.clone());
    work_changed(&app).await;
    Ok(saved)
}

#[tauri::command]
#[specta::specta]
pub async fn schedule_list(state: State<'_, AppState>) -> CmdResult<Vec<WorkSchedule>> {
    Ok(state.data().list_schedules().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn schedule_save(
    app: AppHandle,
    state: State<'_, AppState>,
    id: Option<String>,
    input: WorkScheduleInput,
) -> CmdResult<WorkSchedule> {
    let schedule = state.data().save_schedule(id.as_deref(), input).await?;
    work_changed(&app).await;
    Ok(schedule)
}

#[tauri::command]
#[specta::specta]
pub async fn schedule_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.data().delete_schedule(&id).await?;
    work_changed(&app).await;
    Ok(())
}

// --------------------------------------------------------------------------
// Absences
// --------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn absence_type_list(state: State<'_, AppState>) -> CmdResult<Vec<AbsenceType>> {
    Ok(state.data().list_absence_types().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn absence_type_save(
    app: AppHandle,
    state: State<'_, AppState>,
    id: Option<String>,
    input: AbsenceTypeInput,
) -> CmdResult<AbsenceType> {
    let t = state.data().save_absence_type(id.as_deref(), input).await?;
    work_changed(&app).await;
    Ok(t)
}

#[tauri::command]
#[specta::specta]
pub async fn absence_type_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.data().delete_absence_type(&id).await?;
    work_changed(&app).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn absence_list(
    state: State<'_, AppState>,
    from: NaiveDate,
    to: NaiveDate,
) -> CmdResult<Vec<Absence>> {
    Ok(state.data().list_absences(from, to).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn absence_save(
    app: AppHandle,
    state: State<'_, AppState>,
    id: Option<String>,
    input: AbsenceInput,
) -> CmdResult<Absence> {
    let absence = state.data().save_absence(id.as_deref(), input).await?;
    work_changed(&app).await;
    Ok(absence)
}

#[tauri::command]
#[specta::specta]
pub async fn absence_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.data().delete_absence(&id).await?;
    work_changed(&app).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn vacation_summary(state: State<'_, AppState>, year: i32) -> CmdResult<VacationSummary> {
    Ok(state.data().vacation_summary(year).await?)
}

// --------------------------------------------------------------------------
// Holidays and balance
// --------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn holiday_list(
    state: State<'_, AppState>,
    year: i32,
    lang: String,
) -> CmdResult<Vec<HolidayView>> {
    Ok(state.data().holidays(year, &lang).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn holiday_create(
    app: AppHandle,
    state: State<'_, AppState>,
    date: NaiveDate,
    name: String,
) -> CmdResult<CustomHoliday> {
    let h = state.data().create_custom_holiday(date, &name).await?;
    work_changed(&app).await;
    Ok(h)
}

#[tauri::command]
#[specta::specta]
pub async fn holiday_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.data().delete_custom_holiday(&id).await?;
    work_changed(&app).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn adjustment_list(state: State<'_, AppState>) -> CmdResult<Vec<BalanceAdjustment>> {
    Ok(state.data().list_adjustments().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn adjustment_create(
    app: AppHandle,
    state: State<'_, AppState>,
    input: BalanceAdjustmentInput,
) -> CmdResult<BalanceAdjustment> {
    let a = state.data().create_adjustment(input).await?;
    work_changed(&app).await;
    Ok(a)
}

#[tauri::command]
#[specta::specta]
pub async fn adjustment_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.data().delete_adjustment(&id).await?;
    work_changed(&app).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn work_days(
    state: State<'_, AppState>,
    from: NaiveDate,
    to: NaiveDate,
) -> CmdResult<Vec<WorkDay>> {
    Ok(state.data().work_days(from, to).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn balance(state: State<'_, AppState>) -> CmdResult<Balance> {
    Ok(state.data().balance().await?)
}

// --------------------------------------------------------------------------
// Auto-pause prompts
// --------------------------------------------------------------------------

#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PromptState {
    pub away: Vec<AwayItem>,
    pub morning: bool,
}

#[tauri::command]
#[specta::specta]
pub async fn prompt_state(
    state: State<'_, AppState>,
    activity: State<'_, Activity>,
) -> CmdResult<PromptState> {
    Ok(PromptState {
        away: state.data().pending_away().await?,
        morning: activity.morning_pending(),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn away_resolve(
    app: AppHandle,
    state: State<'_, AppState>,
    pause_id: String,
    resolution: AwayResolution,
) -> CmdResult<()> {
    state.data().resolve_away(&pause_id, resolution).await?;
    changed(&app, &[DataKind::Entry, DataKind::Away]).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn morning_dismiss(app: AppHandle, activity: State<'_, Activity>) -> CmdResult<()> {
    activity.dismiss_morning();
    changed(&app, &[DataKind::Away]).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn prompt_hide(app: AppHandle) {
    windows::hide_prompt(&app);
}

/// Which OS sources the activity watcher uses (e.g. "idle: gnome-mutter").
#[tauri::command]
#[specta::specta]
pub fn activity_source(activity: State<'_, Activity>) -> String {
    activity.source.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

// --------------------------------------------------------------------------
// Timesheet approval (shared workspaces)
// --------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn submissions_list(state: State<'_, AppState>) -> CmdResult<Vec<TimesheetSubmission>> {
    Ok(state.data().list_submissions().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn submission_submit(
    app: AppHandle,
    state: State<'_, AppState>,
    from: NaiveDate,
    to: NaiveDate,
    note: String,
) -> CmdResult<TimesheetSubmission> {
    let submission = state.data().submit_timesheet(from, to, &note).await?;
    work_changed(&app).await;
    Ok(submission)
}

#[tauri::command]
#[specta::specta]
pub async fn submission_decide(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    approve: bool,
    note: String,
) -> CmdResult<TimesheetSubmission> {
    let submission = state.data().decide_submission(&id, approve, &note).await?;
    work_changed(&app).await;
    Ok(submission)
}

#[tauri::command]
#[specta::specta]
pub async fn submission_reopen(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    note: String,
) -> CmdResult<TimesheetSubmission> {
    let submission = state.data().reopen_submission(&id, &note).await?;
    work_changed(&app).await;
    Ok(submission)
}

#[tauri::command]
#[specta::specta]
pub async fn submission_withdraw(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.data().withdraw_submission(&id).await?;
    work_changed(&app).await;
    Ok(())
}

/// Absence requests of other people waiting for a decision.
#[tauri::command]
#[specta::specta]
pub async fn absence_requests(state: State<'_, AppState>) -> CmdResult<Vec<Absence>> {
    Ok(state.data().list_absence_requests().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn absence_decide(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    approve: bool,
    note: String,
) -> CmdResult<Absence> {
    let absence = state.data().decide_absence(&id, approve, &note).await?;
    work_changed(&app).await;
    Ok(absence)
}

/// Time tracked by everyone this device has entries of in the workspace
/// (what the person's role lets the server send).
#[tauri::command]
#[specta::specta]
pub async fn team_summary(
    state: State<'_, AppState>,
    range: zeima_core::summary::Range,
) -> CmdResult<Vec<zeima_core::TeamMember>> {
    Ok(state.data().team_summary(range).await?)
}

// --------------------------------------------------------------------------
// Where people work (shared workspaces)
// --------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn locations_list(
    state: State<'_, AppState>,
    from: NaiveDate,
    to: NaiveDate,
) -> CmdResult<Vec<zeima_core::WorkLocation>> {
    Ok(state.data().list_locations(from, to).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn location_set(
    app: AppHandle,
    state: State<'_, AppState>,
    date: NaiveDate,
    kind: Option<zeima_core::LocationKind>,
) -> CmdResult<()> {
    state.data().set_location(date, kind).await?;
    work_changed(&app).await;
    Ok(())
}
