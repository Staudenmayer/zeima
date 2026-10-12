//! Watches OS activity and applies auto-pause rules (docs/SPEC.md §5.3).
//!
//! Runs on its own thread: the OS probes use blocking D-Bus calls.

use std::sync::Mutex;
use std::time::Duration;

use chrono::{NaiveDate, Timelike};
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;
use time_core::autopause::{AutoPauseConfig, AwayPeriod, Detector, Observation};
use time_core::model::TimerState;
use time_core::summary::Range;
use time_core::worktime::DayWarning;

use crate::commands;
use crate::events::DataKind;
use crate::os;
use crate::state::AppState;
use crate::windows;

const POLL_MS: i64 = 5_000;
/// Check the legal daily maximum every this many polls (1 min).
const LIMIT_CHECK_EVERY: u32 = 12;

#[derive(Default)]
pub struct Activity {
    /// Which OS sources are used (shown in settings).
    pub source: Mutex<String>,
    /// Morning check-in waiting in the prompt window.
    pub morning_pending: Mutex<bool>,
    morning_offered_on: Mutex<Option<NaiveDate>>,
    warned_max_on: Mutex<Option<NaiveDate>>,
}

impl Activity {
    pub fn morning_pending(&self) -> bool {
        *self.morning_pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn dismiss_morning(&self) {
        *self.morning_pending.lock().unwrap_or_else(|e| e.into_inner()) = false;
    }
}

fn auto_pause(app: &AppHandle) -> AutoPauseConfig {
    app.state::<AppState>().work_settings().auto_pause
}

/// Runs once at startup: handles a timer that ran while the app was closed
/// and offers the morning check-in.
pub async fn on_startup(app: &AppHandle) {
    let state = app.state::<AppState>();
    let cfg = auto_pause(app);
    match state.core.check_forgotten_timer(&cfg).await {
        Ok(Some(_)) => {
            commands::changed(app, &[DataKind::Entry, DataKind::Away]).await;
            windows::show_prompt(app);
        }
        Ok(None) => {}
        Err(err) => eprintln!("forgotten timer check failed: {err}"),
    }
    offer_morning(app, &cfg).await;
}

pub fn spawn(app: AppHandle) {
    let result = std::thread::Builder::new()
        .name("activity".into())
        .spawn(move || {
            let mut probe = os::probe();
            let source = probe.describe();
            eprintln!("activity sources: {source}");
            *app.state::<Activity>()
                .source
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = source;

            let mut detector = Detector::new(auto_pause(&app).idle_minutes, POLL_MS);
            let mut polls: u32 = 0;
            loop {
                std::thread::sleep(Duration::from_millis(POLL_MS as u64));
                let cfg = auto_pause(&app);
                detector.set_idle_minutes(cfg.idle_minutes);
                let snapshot = probe.snapshot();
                let now = app.state::<AppState>().core.now_ms();
                let observation = Observation {
                    now,
                    idle_ms: snapshot.idle_ms,
                    locked: snapshot.locked,
                };
                if let Some(period) = detector.feed(observation) {
                    tauri::async_runtime::block_on(on_return(&app, period, &cfg));
                }
                polls = polls.wrapping_add(1);
                if polls.is_multiple_of(LIMIT_CHECK_EVERY) && !detector.is_away() && cfg.warn_daily_max {
                    tauri::async_runtime::block_on(check_daily_limit(&app));
                }
            }
        });
    if let Err(err) = result {
        eprintln!("activity watcher not started: {err}");
    }
}

async fn on_return(app: &AppHandle, period: AwayPeriod, cfg: &AutoPauseConfig) {
    let state = app.state::<AppState>();
    match state.core.handle_away(period, cfg).await {
        Ok(Some(_)) => {
            commands::changed(app, &[DataKind::Entry, DataKind::Away]).await;
            windows::show_prompt(app);
        }
        Ok(None) => {}
        Err(err) => eprintln!("auto-pause failed: {err}"),
    }
    offer_morning(app, cfg).await;
}

/// First activity of the day without a running timer: offer to start.
async fn offer_morning(app: &AppHandle, cfg: &AutoPauseConfig) {
    if !cfg.enabled || !cfg.morning_prompt {
        return;
    }
    let state = app.state::<AppState>();
    let activity = app.state::<Activity>();
    let core = &state.core;
    let (Ok(today), Ok(tz)) = (core.today(), core.tz()) else {
        return;
    };
    let offered_on = |activity: &Activity| {
        *activity
            .morning_offered_on
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    };
    if offered_on(&activity) == Some(today) {
        return;
    }
    let hour = chrono::TimeZone::timestamp_millis_opt(&tz, core.now_ms())
        .single()
        .map(|t| t.hour())
        .unwrap_or(0);
    if !(5..12).contains(&hour) {
        return;
    }
    let idle = matches!(core.timer_state().await, Ok(TimerState::Idle));
    let (from, to) = time_core::worktime::day_bounds(today, tz);
    let tracked_today = core
        .list_entries(Range { from, to })
        .await
        .map(|e| !e.is_empty())
        .unwrap_or(true);
    if !idle || tracked_today {
        return;
    }
    *activity
        .morning_offered_on
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(today);
    *activity.morning_pending.lock().unwrap_or_else(|e| e.into_inner()) = true;
    commands::changed(app, &[DataKind::Away]).await;
    windows::show_prompt(app);
}

async fn check_daily_limit(app: &AppHandle) {
    let state = app.state::<AppState>();
    let activity = app.state::<Activity>();
    let core = &state.core;
    if !matches!(core.timer_state().await, Ok(TimerState::Running { .. })) {
        return;
    }
    let Ok(today) = core.today() else { return };
    if *activity.warned_max_on.lock().unwrap_or_else(|e| e.into_inner()) == Some(today) {
        return;
    }
    let Ok(days) = core.work_days(today, today).await else {
        return;
    };
    let exceeded = days.first().and_then(|d| {
        d.warnings.iter().find_map(|w| match w {
            DayWarning::ExceedsDailyMax { max_minutes } => Some(*max_minutes),
            _ => None,
        })
    });
    if let Some(max) = exceeded {
        *activity.warned_max_on.lock().unwrap_or_else(|e| e.into_inner()) = Some(today);
        let hours = format!("{}:{:02}", max / 60, max % 60);
        let (title, body) = if state.settings().language() == "de" {
            (
                "Tageshöchstarbeitszeit überschritten",
                format!("Du hast heute mehr als {hours} h gearbeitet."),
            )
        } else {
            (
                "Daily maximum exceeded",
                format!("You have worked more than {hours} h today."),
            )
        };
        let _ = app.notification().builder().title(title).body(body).show();
    }
}
