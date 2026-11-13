//! Updating the app from GitHub releases. The release workflow signs every
//! installer with the updater key (`tauri.release.conf.json` turns on the
//! updater artifacts) and publishes `latest.json`; the app checks it against
//! the public key in `tauri.conf.json`. A check is quiet: no network or an
//! unsigned development build simply finds nothing.

use serde::Serialize;
use specta::Type;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

use crate::commands::{CmdError, CmdResult};

#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    /// Release notes, if the release has any.
    pub notes: Option<String>,
}

fn err(e: impl ToString) -> CmdError {
    CmdError::other("update", e)
}

/// The newer version that is available, if any.
#[tauri::command]
#[specta::specta]
pub async fn update_check(app: AppHandle) -> CmdResult<Option<UpdateInfo>> {
    let update = app.updater().map_err(err)?.check().await.map_err(err)?;
    Ok(update.map(|u| UpdateInfo {
        version: u.version.clone(),
        current_version: u.current_version.clone(),
        notes: u.body.clone(),
    }))
}

/// Downloads the newer version, installs it and restarts the app.
#[tauri::command]
#[specta::specta]
pub async fn update_install(app: AppHandle) -> CmdResult<()> {
    let Some(update) = app.updater().map_err(err)?.check().await.map_err(err)? else {
        return Err(err("there is no newer version"));
    };
    update.download_and_install(|_, _| {}, || {}).await.map_err(err)?;
    app.restart()
}
