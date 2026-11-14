//! Watches OS activity and applies auto-pause rules (docs/SPEC.md §5.3).
//!
//! Runs on its own thread: the OS probes use blocking D-Bus calls.

use std::sync::Mutex;
use std::time::Duration;

use chrono::{NaiveDate, Timelike};
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;
use zeima_core::autopause::{AutoPauseConfig, AwayPeriod, Detector, Observation};
use zeima_core::model::TimerState;
use zeima_core::summary::Range;
use zeima_core::worktime::DayWarning;

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
    /// The screen is locked or nobody has touched the machine for a few minutes
    /// (for presence: colleagues see "away").
    idle: std::sync::atomic::AtomicBool,
    /// The shortest auto-pause idle time an organization's policy suggests (0: none).
    policy_idle_minutes: std::sync::atomic::AtomicU32,
}

/// Idle time after which a person counts as away in the presence list.
const AWAY_AFTER_MS: i64 = 5 * 60_000;

impl Activity {
    pub fn set_policy_idle_minutes(&self, minutes: u32) {
        self.policy_idle_minutes
            .store(minutes, std::sync::atomic::Ordering::Relaxed);
    }

    /// The idle time to use: the person's own, or an organization's if that is shorter.
    fn idle_minutes(&self, own: u32) -> u32 {
        match self
            .policy_idle_minutes
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            0 => own,
            policy => own.min(policy).max(1),
        }
    }

    pub fn is_idle(&self) -> bool {
        self.idle.load(std::sync::atomic::Ordering::Relaxed)
    }

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
    // A timer left running in any workspace.
    let mut forgotten = None;
    for core in crate::workspaces::all_cores(&state).await {
        match core.check_forgotten_timer(&cfg).await {
            Ok(Some(found)) => {
                forgotten = Some(Ok(Some(found)));
                break;
            }
            Ok(None) => {}
            Err(err) => forgotten = Some(Err(err)),
        }
    }
    match forgotten.unwrap_or(Ok(None)) {
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
                detector.set_idle_minutes(app.state::<Activity>().idle_minutes(cfg.idle_minutes));
                let snapshot = probe.snapshot();
                let now = app.state::<AppState>().core.now_ms();
                let idle_now =
                    snapshot.locked == Some(true) || snapshot.idle_ms.is_some_and(|ms| ms >= AWAY_AFTER_MS);
                let was_idle = app
                    .state::<Activity>()
                    .idle
                    .swap(idle_now, std::sync::atomic::Ordering::Relaxed);
                // Away (or back) is told to the organizations at once.
                if idle_now != was_idle {
                    crate::presence::report_now(&app);
                }
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
    let core = crate::workspaces::running_core(&state).await;
    match core.handle_away(period, cfg).await {
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
    let core_arc = crate::workspaces::running_core(&state).await;
    let core = &core_arc;
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
    let (from, to) = zeima_core::worktime::day_bounds(today, tz);
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
    let core_arc = crate::workspaces::running_core(&state).await;
    let core = &core_arc;
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
