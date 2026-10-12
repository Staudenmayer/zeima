//! Notifies when a project reaches its budget warning threshold or 100 %,
//! once per threshold and budget period.

use std::collections::HashMap;

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::state::AppState;
use crate::tray::format_elapsed;

/// `local_meta` key: project id → `"<period start>|<level>"` last notified.
const META_KEY: &str = "budget_notified";

pub async fn check(app: &AppHandle) {
    if let Err(err) = check_inner(app).await {
        eprintln!("budget check failed: {err}");
    }
}

async fn check_inner(app: &AppHandle) -> time_core::Result<()> {
    let state = app.state::<AppState>();
    let core = &state.core;
    let statuses = core.budget_statuses().await?;
    let mut notified: HashMap<String, String> = core
        .get_meta(META_KEY)
        .await?
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    let mut dirty = false;
    let de = state.settings().language() == "de";

    for status in &statuses {
        let level = status.level();
        if level == 0 {
            continue;
        }
        let period = status
            .period_start
            .map_or_else(|| "total".to_string(), |d| d.to_string());
        let already = notified.get(&status.project_id).and_then(|v| {
            let (p, l) = v.split_once('|')?;
            (p == period).then(|| l.parse::<i64>().ok()).flatten()
        });
        if already.is_some_and(|l| l >= level) {
            continue;
        }
        notified.insert(status.project_id.clone(), format!("{period}|{level}"));
        dirty = true;

        let name = core
            .get_project(&status.project_id)
            .await
            .map(|p| p.name)
            .unwrap_or_default();
        let used = format_elapsed(status.used_ms);
        let budget = format_elapsed(status.budget_ms);
        let (title, body) = match (level >= 100, de) {
            (true, true) => (
                format!("Budget von {name} aufgebraucht"),
                format!("{used} von {budget} h erfasst."),
            ),
            (true, false) => (
                format!("{name} is over budget"),
                format!("{used} of {budget} h tracked."),
            ),
            (false, true) => (
                format!("{name}: {level} % des Budgets"),
                format!("{used} von {budget} h erfasst."),
            ),
            (false, false) => (
                format!("{name}: {level} % of budget used"),
                format!("{used} of {budget} h tracked."),
            ),
        };
        let _ = app.notification().builder().title(title).body(body).show();
    }

    if dirty {
        let json = serde_json::to_string(&notified)?;
        core.set_meta(META_KEY, &json).await?;
    }
    Ok(())
}
