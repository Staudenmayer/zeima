//! Cost centers, reports, exports and backups.

use std::path::PathBuf;

use chrono::Datelike;
use rust_xlsxwriter::{ExcelDateTime, Format, Workbook};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use zeima_core::export::{entries_table, summary_table, to_csv, Cell, Lookup, Table};
use zeima_core::model::{CostCenter, CostCenterAssignment, CostCenterAssignmentInput, CostCenterInput};
use zeima_core::report::{Report, ReportQuery};
use zeima_core::BudgetStatus;

use crate::backup;
use crate::commands::{changed, CmdError, CmdResult};
use crate::events::DataKind;
use crate::state::AppState;

// --------------------------------------------------------------------------
// Cost centers
// --------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn cost_center_list(state: State<'_, AppState>) -> CmdResult<Vec<CostCenter>> {
    Ok(state.core.list_cost_centers().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn cost_center_save(
    app: AppHandle,
    state: State<'_, AppState>,
    id: Option<String>,
    input: CostCenterInput,
) -> CmdResult<CostCenter> {
    let cc = state.core.save_cost_center(id.as_deref(), input).await?;
    changed(&app, &[DataKind::CostCenter]).await;
    Ok(cc)
}

#[tauri::command]
#[specta::specta]
pub async fn cost_center_delete(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.core.delete_cost_center(&id).await?;
    changed(&app, &[DataKind::CostCenter]).await;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn cost_center_assignment_list(state: State<'_, AppState>) -> CmdResult<Vec<CostCenterAssignment>> {
    Ok(state.core.list_cost_center_assignments().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn cost_center_assignment_save(
    app: AppHandle,
    state: State<'_, AppState>,
    id: Option<String>,
    input: CostCenterAssignmentInput,
) -> CmdResult<CostCenterAssignment> {
    let a = state
        .core
        .save_cost_center_assignment(id.as_deref(), input)
        .await?;
    changed(&app, &[DataKind::CostCenter]).await;
    Ok(a)
}

#[tauri::command]
#[specta::specta]
pub async fn cost_center_assignment_delete(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<()> {
    state.core.delete_cost_center_assignment(&id).await?;
    changed(&app, &[DataKind::CostCenter]).await;
    Ok(())
}

// --------------------------------------------------------------------------
// Reports and budgets
// --------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn report(state: State<'_, AppState>, query: ReportQuery) -> CmdResult<Report> {
    Ok(state.core.report(&query).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn budget_list(state: State<'_, AppState>) -> CmdResult<Vec<BudgetStatus>> {
    Ok(state.core.budget_statuses().await?)
}

// --------------------------------------------------------------------------
// Exports
// --------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    /// Summary and entries on two sheets.
    Xlsx,
    /// Entries only.
    CsvEntries,
    /// Grouped totals only.
    CsvSummary,
}

/// Asks where to save; `None` when the user cancels.
pub(crate) async fn ask_save_path(app: &AppHandle, file_name: &str, filter: (&str, &str)) -> Option<PathBuf> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app
        .dialog()
        .file()
        .set_file_name(file_name)
        .add_filter(filter.0, &[filter.1]);
    if let Ok(dir) = app.path().download_dir() {
        dialog = dialog.set_directory(dir);
    }
    dialog.save_file(move |path| {
        let _ = tx.send(path);
    });
    rx.await.ok().flatten().and_then(|p| p.into_path().ok())
}

pub(crate) fn write_file(path: &PathBuf, bytes: &[u8]) -> CmdResult<()> {
    std::fs::write(path, bytes).map_err(|e| CmdError::other("io", e))
}

/// Writes the report to a file the user picks. Returns the path, or
/// `None` when cancelled. `file_stem` is the suggested name without extension.
#[tauri::command]
#[specta::specta]
pub async fn report_export(
    app: AppHandle,
    state: State<'_, AppState>,
    query: ReportQuery,
    format: ExportFormat,
    file_stem: String,
) -> CmdResult<Option<String>> {
    let core = &state.core;
    let formats = state.settings().formats();
    let report = core.report(&query).await?;
    let activities = core.list_activities(true).await?;
    let projects = core.list_projects(true).await?;
    let tags = core.list_tags().await?;
    let cost_centers = core.list_cost_centers().await?;
    let lookup = Lookup::new(&activities, &projects, &tags, &cost_centers);
    let tz = core.tz()?;
    let summary = || summary_table(&report, &query.group_by, &lookup, &formats);
    let entries = || entries_table(&report, &lookup, tz, core.now_ms(), &formats);

    let (ext, filter, bytes) = match format {
        ExportFormat::Xlsx => (
            "xlsx",
            "Excel",
            to_xlsx(&[summary(), entries()], formats.excel_date())?,
        ),
        ExportFormat::CsvEntries => ("csv", "CSV", to_csv(&entries(), &formats).into_bytes()),
        ExportFormat::CsvSummary => ("csv", "CSV", to_csv(&summary(), &formats).into_bytes()),
    };
    let Some(path) = ask_save_path(&app, &format!("{file_stem}.{ext}"), (filter, ext)).await else {
        return Ok(None);
    };
    write_file(&path, &bytes)?;
    Ok(Some(path.display().to_string()))
}

fn to_xlsx(tables: &[Table], date_format: &str) -> CmdResult<Vec<u8>> {
    let err = |e: rust_xlsxwriter::XlsxError| CmdError::other("xlsx", e);
    let mut workbook = Workbook::new();
    let bold = Format::new().set_bold();
    let hours = Format::new().set_num_format("0.00");
    let date = Format::new().set_num_format(date_format);
    for table in tables {
        let sheet = workbook.add_worksheet();
        // Sheet names are limited to 31 characters.
        sheet
            .set_name(table.name.chars().take(31).collect::<String>())
            .map_err(err)?;
        for (col, header) in table.headers.iter().enumerate() {
            sheet
                .write_string_with_format(0, col as u16, header, &bold)
                .map_err(err)?;
        }
        for (i, row) in table.rows.iter().enumerate() {
            let r = i as u32 + 1;
            for (col, cell) in row.iter().enumerate() {
                let c = col as u16;
                match cell {
                    Cell::Text(s) => sheet.write_string(r, c, s).map(|_| ()),
                    Cell::Hours(h) => sheet.write_number_with_format(r, c, *h, &hours).map(|_| ()),
                    Cell::Int(n) => sheet.write_number(r, c, *n as f64).map(|_| ()),
                    Cell::Date(d) => {
                        let value = ExcelDateTime::from_ymd(d.year() as u16, d.month() as u8, d.day() as u8)
                            .map_err(err)?;
                        sheet.write_datetime_with_format(r, c, &value, &date).map(|_| ())
                    }
                }
                .map_err(err)?;
            }
        }
        sheet.set_freeze_panes(1, 0).map_err(err)?;
        sheet.autofit();
    }
    workbook.save_to_buffer().map_err(err)
}

/// Full data export as JSON (SPEC §11).
#[tauri::command]
#[specta::specta]
pub async fn data_export_json(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Option<String>> {
    let json = state.core.export_json().await?;
    let bytes = serde_json::to_vec_pretty(&json).map_err(|e| CmdError::other("json", e))?;
    let name = format!("time-export-{}.json", state.core.today()?);
    let Some(path) = ask_save_path(&app, &name, ("JSON", "json")).await else {
        return Ok(None);
    };
    write_file(&path, &bytes)?;
    Ok(Some(path.display().to_string()))
}

// --------------------------------------------------------------------------
// Backups
// --------------------------------------------------------------------------

#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub dir: String,
    /// File names, newest first.
    pub files: Vec<String>,
}

#[tauri::command]
#[specta::specta]
pub async fn backup_info(app: AppHandle) -> CmdResult<BackupInfo> {
    let dir = backup::dir(&app).map_err(|e| CmdError::other("io", e))?;
    Ok(BackupInfo {
        dir: dir.display().to_string(),
        files: backup::list(&dir),
    })
}

#[tauri::command]
#[specta::specta]
pub async fn backup_now(app: AppHandle) -> CmdResult<BackupInfo> {
    backup::run(&app, true)
        .await
        .map_err(|e| CmdError::other("backup", e))?;
    backup_info(app).await
}

#[tauri::command]
#[specta::specta]
pub async fn backup_open_dir(app: AppHandle) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = backup::dir(&app).map_err(|e| CmdError::other("io", e))?;
    app.opener()
        .open_path(dir.display().to_string(), None::<&str>)
        .map_err(|e| CmdError::other("opener", e))
}
