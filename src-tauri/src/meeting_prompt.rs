//! When a meeting the person took part in ends and nothing is tracked for it,
//! a notification asks whether to track it (the timesheet shows the meeting;
//! one click there makes the entry). Runs from the minute tick.

use std::sync::atomic::{AtomicI64, Ordering};

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;
use zeima_core::summary::Range;
use zeima_core::RsvpStatus;

use crate::state::AppState;

/// How far back to look the first time (a meeting that ended a moment ago still counts).
const FIRST_LOOK_BACK_MS: i64 = 5 * 60_000;
/// Meetings that ended longer ago than this are no news.
const MAX_AGE_MS: i64 = 30 * 60_000;

static LAST: AtomicI64 = AtomicI64::new(0);

pub async fn check(app: &AppHandle) {
    let state = app.state::<AppState>();
    let now = state.core.now_ms();
    let since = match LAST.swap(now, Ordering::Relaxed) {
        0 => now - FIRST_LOOK_BACK_MS,
        last => last.max(now - MAX_AGE_MS),
    };
    let de = state.settings().language() == "de";
    for core in crate::workspaces::all_cores(&state)
        .await
        .into_iter()
        .filter(|c| c.ctx().shared)
    {
        // Meetings that started up to a day ago may have ended since.
        let Ok(found) = core.meetings_between(since - 86_400_000, now).await else {
            continue;
        };
        for o in found.into_iter().filter(|o| o.end_at > since && o.end_at <= now) {
            let took_part =
                o.meeting.user_id == core.ctx().user_id || o.my_status == Some(RsvpStatus::Accepted);
            if o.meeting.cancelled || !took_part {
                continue;
            }
            let tracked = core
                .list_entries(Range {
                    from: o.start_at,
                    to: o.end_at,
                })
                .await
                .map(|e| !e.is_empty())
                .unwrap_or(true);
            if tracked {
                continue;
            }
            let (title, body) = if de {
                (
                    "Termin beendet".to_string(),
                    format!(
                        "\"{}\" erfassen? Klicke in der Zeiterfassung auf den Termin.",
                        o.meeting.title
                    ),
                )
            } else {
                (
                    "Meeting over".to_string(),
                    format!(
                        "Track \"{}\"? Click the meeting in the timesheet.",
                        o.meeting.title
                    ),
                )
            };
            let _ = app.notification().builder().title(title).body(body).show();
        }
    }
}
