//! IPC commands for work time (country profile, schedules, absences,
//! holidays, balance) and auto-pause prompts.

use chrono::NaiveDate;
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, State};
use time_core::jurisdiction::{self, ProfileInfo};
use time_core::model::{
    Absence, AbsenceInput, AbsenceType, AbsenceTypeInput, BalanceAdjustment, BalanceAdjustmentInput,
    CustomHoliday, JurisdictionSelection, WorkSettings,
};
use time_core::worktime::{WorkDay, WorkSchedule};
use time_core::{
    AwayItem, AwayResolution, Balance, HolidayView, ProfileSummary, VacationSummary, WorkScheduleInput,
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
    Ok(state.core.profile_summary(&lang).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn jurisdiction_get(state: State<'_, AppState>) -> CmdResult<Option<JurisdictionSelection>> {
    Ok(state.core.jurisdiction().await?)
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
        .core
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
    Ok(state.core.work_settings().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn work_settings_set(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: WorkSettings,
) -> CmdResult<WorkSettings> {
    let saved = state.core.set_work_settings(settings).await?;
    state.set_work_settings(saved.clone());
    work_changed(&app).await;
    Ok(saved)
}

#[tauri::command]
#[specta::specta]
pub async fn schedule_list(state: State<'_, AppState>) -> CmdResult<Vec<WorkSchedule>> {
    Ok(state.core.list_schedules().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn schedule_save(
    app: AppHandle,
    state: State<'_, AppState>,
    id: Option<String>,
    input: WorkScheduleInput,
) -> CmdResult<WorkSchedule> {
    let schedule = state.core.save_schedule(id.as_deref(), input).await?;
    work_changed(&app).await;
    Ok(schedule)
}

#[tauri::command]
#[specta::specta]
pub async fn schedule_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.core.delete_schedule(&id).await?;
    work_changed(&app).await;
    Ok(())
}

// --------------------------------------------------------------------------
// Absences
// --------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn absence_type_list(state: State<'_, AppState>) -> CmdResult<Vec<AbsenceType>> {
    Ok(state.core.list_absence_types().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn absence_type_save(
    app: AppHandle,
    state: State<'_, AppState>,
    id: Option<String>,
    input: AbsenceTypeInput,
) -> CmdResult<AbsenceType> {
    let t = state.core.save_absence_type(id.as_deref(), input).await?;
    work_changed(&app).await;
    Ok(t)
}

#[tauri::command]
#[specta::specta]
pub async fn absence_type_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.core.delete_absence_type(&id).await?;
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
    Ok(state.core.list_absences(from, to).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn absence_save(
    app: AppHandle,
    state: State<'_, AppState>,
    id: Option<String>,
    input: AbsenceInput,
) -> CmdResult<Absence> {
    let absence = state.core.save_absence(id.as_deref(), input).await?;
    work_changed(&app).await;
    Ok(absence)
}

#[tauri::command]
#[specta::specta]
pub async fn absence_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.core.delete_absence(&id).await?;
    work_changed(&app).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn vacation_summary(state: State<'_, AppState>, year: i32) -> CmdResult<VacationSummary> {
    Ok(state.core.vacation_summary(year).await?)
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
    Ok(state.core.holidays(year, &lang).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn holiday_create(
    app: AppHandle,
    state: State<'_, AppState>,
    date: NaiveDate,
    name: String,
) -> CmdResult<CustomHoliday> {
    let h = state.core.create_custom_holiday(date, &name).await?;
    work_changed(&app).await;
    Ok(h)
}

#[tauri::command]
#[specta::specta]
pub async fn holiday_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.core.delete_custom_holiday(&id).await?;
    work_changed(&app).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn adjustment_list(state: State<'_, AppState>) -> CmdResult<Vec<BalanceAdjustment>> {
    Ok(state.core.list_adjustments().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn adjustment_create(
    app: AppHandle,
    state: State<'_, AppState>,
    input: BalanceAdjustmentInput,
) -> CmdResult<BalanceAdjustment> {
    let a = state.core.create_adjustment(input).await?;
    work_changed(&app).await;
    Ok(a)
}

#[tauri::command]
#[specta::specta]
pub async fn adjustment_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.core.delete_adjustment(&id).await?;
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
    Ok(state.core.work_days(from, to).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn balance(state: State<'_, AppState>) -> CmdResult<Balance> {
    Ok(state.core.balance().await?)
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
        away: state.core.pending_away().await?,
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
    state.core.resolve_away(&pause_id, resolution).await?;
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
