//! While a timer runs and a meeting the person organizes or accepted starts,
//! tracking moves to the meeting's activity, project and tags; when the meeting
//! ends, tracking goes back to what was running (SPEC §5.8).
//!
//! Only a timer that was started before the meeting is followed: starting or
//! changing the timer during the meeting is the person's own choice and is left
//! alone, and so is a timer they stop (it is not started again afterwards).

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;
use zeima_core::model::{StartTimer, TimerState};
use zeima_core::{Core, RsvpStatus};

use crate::events::DataKind;
use crate::state::AppState;

const EVERY: Duration = Duration::from_secs(10);
/// `local_meta` key: what to go back to, so a restart in the middle of a meeting does not lose it.
const KEY: &str = "meeting_follow";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Follow {
    meeting_title: String,
    /// When the meeting ends (unix ms).
    ends_at: i64,
    /// Where the meeting's entry runs, and which entry it is.
    meeting_workspace: String,
    entry_id: String,
    /// What was running before the meeting, and where.
    origin_workspace: String,
    origin: StartTimer,
}

async fn load(state: &AppState) -> Option<Follow> {
    let text = state.core.get_meta(KEY).await.ok().flatten()?;
    serde_json::from_str(&text).ok()
}

async fn save(state: &AppState, follow: Option<&Follow>) {
    let text = follow
        .and_then(|f| serde_json::to_string(f).ok())
        .unwrap_or_default();
    let _ = state.core.set_meta(KEY, &text).await;
}

fn in_workspace(cores: &[Arc<Core>], id: &str) -> Option<Arc<Core>> {
    cores.iter().find(|c| c.ctx().workspace_id == id).cloned()
}

/// Whether a meeting that has started takes over the running timer: the person is
/// in it, it is on, and the timer was started before the meeting (a timer started
/// during the meeting is the person's own choice).
fn takes_over(running_since: i64, meeting_start: i64, took_part: bool, cancelled: bool) -> bool {
    took_part && !cancelled && running_since < meeting_start
}

fn say(app: &AppHandle, de: bool, title_en: &str, title_de: &str, body: String) {
    let title = if de { title_de } else { title_en };
    let _ = app.notification().builder().title(title).body(body).show();
}

pub async fn check(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let de = settings.language() == "de";
    let cores = crate::workspaces::all_cores(&state).await;
    let now = state.core.now_ms();

    // In a meeting already: go back when it is over, or let go if the person took over.
    if let Some(follow) = load(&state).await {
        let Some(meeting_core) = in_workspace(&cores, &follow.meeting_workspace) else {
            save(&state, None).await;
            return;
        };
        let still_ours = matches!(meeting_core.timer_state().await, Ok(TimerState::Running { entry }) if entry.id == follow.entry_id);
        if !still_ours {
            save(&state, None).await;
            return;
        }
        if now < follow.ends_at {
            return;
        }
        save(&state, None).await;
        let Some(origin_core) = in_workspace(&cores, &follow.origin_workspace) else {
            return;
        };
        if origin_core.ctx().workspace_id != meeting_core.ctx().workspace_id {
            let _ = meeting_core.stop_timer().await;
        }
        match origin_core.start_timer(follow.origin.clone()).await {
            Ok(_) => {
                crate::commands::changed(app, &[DataKind::Entry]).await;
                say(
                    app,
                    de,
                    "Meeting over",
                    "Termin beendet",
                    if de {
                        format!(
                            "\"{}\" ist vorbei. Die Zeiterfassung läuft wie vorher weiter.",
                            follow.meeting_title
                        )
                    } else {
                        format!(
                            "\"{}\" is over. Tracking is back on what you were doing.",
                            follow.meeting_title
                        )
                    },
                );
            }
            Err(err) => eprintln!("could not go back after the meeting: {err}"),
        }
        return;
    }

    if !settings.follow_meetings {
        return;
    }
    // A timer that is running, wherever it runs.
    let mut running = None;
    for core in &cores {
        if let Ok(TimerState::Running { entry }) = core.timer_state().await {
            running = Some((core.clone(), entry));
            break;
        }
    }
    let Some((running_core, running_entry)) = running else {
        return;
    };
    for core in cores.iter().filter(|c| c.ctx().shared) {
        let Ok(found) = core.meetings_between(now, now + 1).await else {
            continue;
        };
        for o in found {
            let took_part =
                o.meeting.user_id == core.ctx().user_id || o.my_status == Some(RsvpStatus::Accepted);
            if !takes_over(running_entry.start_at, o.start_at, took_part, o.meeting.cancelled) {
                continue;
            }
            // The project of a meeting someone else made may be one this person cannot use (the server does not even
            // send it to them): then the meeting's time goes on no project, never on that one.
            let usable_project = match &o.meeting.project_id {
                Some(id) => core.get_project(id).await.ok().map(|p| p.id),
                None => None,
            };
            let hidden = o.meeting.project_id.is_some() && usable_project.is_none();
            let origin = StartTimer {
                activity_id: running_entry.activity_id.clone(),
                project_id: running_entry.project_id.clone(),
                tag_ids: running_entry.tag_ids.clone(),
                note: running_entry.note.clone(),
                billable: Some(running_entry.billable),
            };
            let start = StartTimer {
                activity_id: o
                    .meeting
                    .activity_id
                    .clone()
                    .or_else(|| running_entry.activity_id.clone()),
                project_id: usable_project,
                tag_ids: if hidden {
                    Vec::new()
                } else {
                    o.meeting.tag_ids.clone()
                },
                note: o.meeting.title.clone(),
                billable: Some(o.meeting.billable),
            };
            // One timer at a time: one in another workspace is stopped first.
            if running_core.ctx().workspace_id != core.ctx().workspace_id
                && running_core.stop_timer().await.is_err()
            {
                continue;
            }
            match core.start_timer(start).await {
                Ok(TimerState::Running { entry }) => {
                    save(
                        &state,
                        Some(&Follow {
                            meeting_title: o.meeting.title.clone(),
                            ends_at: o.end_at,
                            meeting_workspace: core.ctx().workspace_id.clone(),
                            entry_id: entry.id,
                            origin_workspace: running_core.ctx().workspace_id.clone(),
                            origin,
                        }),
                    )
                    .await;
                    crate::commands::changed(app, &[DataKind::Entry]).await;
                    say(
                        app,
                        de,
                        "Meeting started",
                        "Termin begonnen",
                        if de {
                            format!(
                                "Die Zeiterfassung ist jetzt auf \"{}\" und kehrt danach zurück.",
                                o.meeting.title
                            )
                        } else {
                            format!(
                                "Tracking is now on \"{}\" and goes back afterwards.",
                                o.meeting.title
                            )
                        },
                    );
                    return;
                }
                Ok(_) => {}
                Err(err) => eprintln!("could not follow the meeting: {err}"),
            }
        }
    }
}

/// Looks every few seconds, so a meeting is followed within moments of its start.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            check(&app).await;
            tokio::time::sleep(EVERY).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::takes_over;

    #[test]
    fn a_meeting_takes_over_only_a_timer_started_before_it() {
        assert!(takes_over(100, 200, true, false));
        assert!(!takes_over(200, 200, true, false), "started with the meeting");
        assert!(!takes_over(300, 200, true, false), "started during the meeting");
        assert!(!takes_over(100, 200, false, false), "not asked or not accepted");
        assert!(!takes_over(100, 200, true, true), "called off");
    }
}
