//! Automatic daily database backups (keep 14) and the start-up integrity
//! check (docs/SPEC.md §9).

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::state::AppState;

const KEEP: usize = 14;
const PREFIX: &str = "time-";
const SUFFIX: &str = ".db";

pub fn dir(app: &AppHandle) -> std::io::Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(std::io::Error::other)?
        .join("backups");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Backup file names, newest first (names sort by date).
pub fn list(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with(PREFIX) && n.ends_with(SUFFIX))
        .collect();
    names.sort_unstable_by(|a, b| b.cmp(a));
    names
}

/// Backs up today's database unless that already happened (`force`
/// replaces today's backup), then deletes all but the newest [`KEEP`].
pub async fn run(app: &AppHandle, force: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let dir = dir(app).map_err(|e| e.to_string())?;
    let today = state.core.today().map_err(|e| e.to_string())?;
    let target = dir.join(format!("{PREFIX}{today}{SUFFIX}"));
    if target.exists() && !force {
        return Ok(());
    }
    // VACUUM INTO needs a fresh file; write next to it, then replace.
    let tmp = dir.join(format!("{PREFIX}{today}{SUFFIX}.tmp"));
    let _ = std::fs::remove_file(&tmp);
    state.core.backup_to(&tmp).await.map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
    for old in list(&dir).into_iter().skip(KEEP) {
        let _ = std::fs::remove_file(dir.join(old));
    }
    Ok(())
}

/// Integrity check, then the first backup of the day. A damaged database
/// is not backed up, so the good backups are kept.
pub async fn on_startup(app: &AppHandle) {
    let state = app.state::<AppState>();
    match state.core.integrity_problem().await {
        Ok(None) => {
            if let Err(err) = run(app, false).await {
                eprintln!("backup failed: {err}");
            }
        }
        Ok(Some(problem)) => {
            eprintln!("database integrity check failed: {problem}");
            let de = state.settings().language() == "de";
            let (title, body) = if de {
                ("Datenbank beschädigt", "Die Datenbank hat die Integritätsprüfung nicht bestanden. Sicherungen liegen im Ordner „backups“.")
            } else {
                (
                    "Database damaged",
                    "The database failed its integrity check. Backups are in the \"backups\" folder.",
                )
            };
            let _ = app.notification().builder().title(title).body(body).show();
        }
        Err(err) => eprintln!("integrity check failed to run: {err}"),
    }
}
