//! Periodic work while the app runs: timer heartbeat, the elapsed time
//! shown in the tray, budget warnings, the daily backup, and the addon
//! hooks `schedule.tick` and `day.closed`.

use std::time::Duration;

use chrono::{Duration as Days, NaiveDate};
use tauri::{AppHandle, Manager};
use time_core::events::DomainEvent;
use time_core::model::TimerState;

use crate::state::AppState;
use crate::{addons, backup, budget, tray};

const INTERVAL: Duration = Duration::from_secs(30);
/// Ticks between backup checks (one hour).
const BACKUP_EVERY: u32 = 120;

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(INTERVAL);
        let mut ticks: u32 = 0;
        loop {
            ticker.tick().await;
            ticks = ticks.wrapping_add(1);
            // The first backup runs after the start-up integrity check.
            if ticks.is_multiple_of(BACKUP_EVERY) {
                if let Err(err) = backup::run(&app, false).await {
                    eprintln!("backup failed: {err}");
                }
            }
            let state = app.state::<AppState>();
            close_days(&app).await;
            app.state::<addons::Addons>().tick(state.core.now_ms()).await;
            if let Ok(TimerState::Running { .. }) = state.core.timer_state().await {
                if let Err(err) = state.core.heartbeat().await {
                    eprintln!("heartbeat failed: {err}");
                }
                tray::refresh(&app).await;
                budget::check(&app).await;
            }
        }
    });
}

/// `local_meta` key: the last date `day.closed` was published for.
const DAY_CLOSED_KEY: &str = "day_closed_until";
/// Missed days published at most (e.g. after a long time switched off).
const MAX_CATCH_UP_DAYS: i64 = 7;

/// Publishes `day.closed` once for each day that has ended since the last
/// run. The first run only records yesterday, so installing the app does
/// not replay history.
async fn close_days(app: &AppHandle) {
    let core = &app.state::<AppState>().core;
    let Ok(today) = core.today() else { return };
    let yesterday = today - Days::days(1);
    let last = core
        .get_meta(DAY_CLOSED_KEY)
        .await
        .ok()
        .flatten()
        .and_then(|d| d.parse::<NaiveDate>().ok());
    let Some(last) = last else {
        let _ = core.set_meta(DAY_CLOSED_KEY, &yesterday.to_string()).await;
        return;
    };
    if last >= yesterday {
        return;
    }
    let from = (last + Days::days(1)).max(yesterday - Days::days(MAX_CATCH_UP_DAYS - 1));
    match core.work_days(from, yesterday).await {
        Ok(days) => {
            for day in days {
                core.publish(DomainEvent::DayClosed { date: day.date, day });
            }
        }
        Err(err) => eprintln!("day.closed: {err}"),
    }
    let _ = core.set_meta(DAY_CLOSED_KEY, &yesterday.to_string()).await;
}
