//! Tells the organizations the person belongs to whether they are online,
//! tracking or away (SPEC §5.10), and when somebody else's presence changes.
//!
//! Privacy: an organization hears "tracking" only for a timer that runs in
//! its own workspace; a timer in the personal workspace or another
//! organization shows as plain "online". The project is sent only if the
//! person switched on `share_project_presence`.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};
use zeima_core::model::TimerState;

use crate::state::AppState;

/// How often to look; a report that has not changed is repeated about every minute.
const CHECK_EVERY: Duration = Duration::from_secs(15);
const REPEAT_AFTER: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Report {
    state: &'static str,
    project_id: Option<String>,
}

/// What an organization hears, given the timer that runs in its workspace (if any).
fn report_for(timer: Option<&TimerState>, share_project: bool) -> Report {
    match timer {
        Some(TimerState::Running { entry }) => Report {
            state: "tracking",
            project_id: entry.project_id.clone().filter(|_| share_project),
        },
        Some(TimerState::Paused { .. }) => Report {
            state: "away",
            project_id: None,
        },
        _ => Report {
            state: "online",
            project_id: None,
        },
    }
}

async fn check(app: &AppHandle, last: &mut HashMap<String, (Report, Instant)>) {
    let state = app.state::<AppState>();
    // Only while signed in; the server forgets those who stop reporting.
    if !matches!(crate::account::current_token(app).await, Ok(Some(_))) {
        last.clear();
        return;
    }
    let Ok(workspaces) = state.core.workspaces().await else {
        return;
    };
    let Ok(Some(user)) = state.core.get_meta(crate::workspaces::ACCOUNT_USER_KEY).await else {
        return;
    };
    let share = state.settings().share_project_presence;
    for w in workspaces.into_iter().filter(|w| w.kind == "org") {
        let Some(org_id) = w.org_id.clone() else { continue };
        let timer = state.core.scoped(&w.id, &user).timer_state().await.ok();
        let report = report_for(timer.as_ref(), share);
        let due = last
            .get(&org_id)
            .is_none_or(|(sent, at)| *sent != report || at.elapsed() >= REPEAT_AFTER);
        if !due {
            continue;
        }
        let body = serde_json::json!({ "state": report.state, "projectId": report.project_id });
        let sent = crate::org::org_request(
            app.clone(),
            "PUT".into(),
            format!("/v1/orgs/{org_id}/presence"),
            Some(body.to_string()),
        )
        .await;
        if sent.is_ok() {
            last.insert(org_id, (report, Instant::now()));
        }
    }
}

/// Reports presence while the app runs.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last = HashMap::new();
        loop {
            check(&app, &mut last).await;
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use zeima_core::model::{Pause, TimeEntry};

    use super::*;

    fn entry(project: Option<&str>) -> TimeEntry {
        serde_json::from_value(serde_json::json!({
            "id": "e", "workspaceId": "w", "userId": "u", "activityId": null, "projectId": project,
            "startAt": 1, "endAt": null, "tz": "UTC", "note": "", "billable": false, "tagIds": [],
            "costCenterId": null, "source": "timer", "locked": false, "createdAt": 1, "updatedAt": 1
        }))
        .unwrap()
    }

    #[test]
    fn tracking_names_the_project_only_when_shared() {
        let running = TimerState::Running {
            entry: entry(Some("p1")),
        };
        assert_eq!(
            report_for(Some(&running), false),
            Report {
                state: "tracking",
                project_id: None
            }
        );
        assert_eq!(
            report_for(Some(&running), true),
            Report {
                state: "tracking",
                project_id: Some("p1".into())
            }
        );
    }

    #[test]
    fn idle_or_elsewhere_is_just_online_and_a_pause_is_away() {
        assert_eq!(
            report_for(None, true),
            Report {
                state: "online",
                project_id: None
            }
        );
        assert_eq!(report_for(Some(&TimerState::Idle), true).state, "online");
        let pause: Pause = serde_json::from_value(serde_json::json!({
            "id": "p", "workspaceId": "w", "userId": "u", "startAt": 1, "endAt": null, "kind": "manual",
            "resumeEntryId": null, "followupEntryId": null, "applied": false, "accepted": null,
            "createdAt": 1, "updatedAt": 1
        }))
        .unwrap();
        let paused = TimerState::Paused {
            pause,
            resume_entry: None,
        };
        assert_eq!(
            report_for(Some(&paused), true),
            Report {
                state: "away",
                project_id: None
            }
        );
    }
}
