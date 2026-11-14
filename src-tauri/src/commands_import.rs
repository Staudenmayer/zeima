//! Importing time entries from Toggl, Clockify and similar CSV exports.

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use zeima_core::import::{parse_import, DateOrder};
use zeima_core::{ImportPreview, ImportReport};

use crate::commands::{changed, CmdError, CmdResult};
use crate::events::DataKind;
use crate::state::AppState;

/// More than this is not a time export.
const MAX_BYTES: u64 = 25 * 1024 * 1024;

#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PickedFile {
    pub name: String,
    pub text: String,
}

/// Lets the person choose an export file and reads it.
#[tauri::command]
#[specta::specta]
pub async fn import_pick(app: AppHandle) -> CmdResult<Option<PickedFile>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("CSV", &["csv", "txt"])
        .pick_file(move |path| {
            let _ = tx.send(path);
        });
    let Some(path) = rx.await.ok().flatten().and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let size = std::fs::metadata(&path)
        .map_err(|e| CmdError::other("io", e))?
        .len();
    if size > MAX_BYTES {
        return Err(CmdError::other(
            "validation",
            "this file is too big for a time export",
        ));
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|_| CmdError::other("validation", "the file is not text (a CSV export is expected)"))?;
    Ok(Some(PickedFile {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        text,
    }))
}

/// What importing `text` would do.
#[tauri::command]
#[specta::specta]
pub async fn import_preview(
    state: State<'_, AppState>,
    text: String,
    order: DateOrder,
) -> CmdResult<ImportPreview> {
    let core = state.data();
    let parsed = parse_import(&text, order, &core.default_tz())?;
    Ok(core.preview_import(&parsed).await?)
}

/// Adds the entries of `text` (projects and tags as needed).
#[tauri::command]
#[specta::specta]
pub async fn import_run(
    app: AppHandle,
    state: State<'_, AppState>,
    text: String,
    order: DateOrder,
) -> CmdResult<ImportReport> {
    let core = state.data();
    let parsed = parse_import(&text, order, &core.default_tz())?;
    let report = core.import_entries(&parsed).await?;
    changed(
        &app,
        &[
            DataKind::Project,
            DataKind::Tag,
            DataKind::Activity,
            DataKind::Entry,
        ],
    )
    .await;
    Ok(report)
}
