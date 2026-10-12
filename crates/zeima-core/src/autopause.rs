//! Auto-pause: detects periods away from the computer (idle, locked,
//! suspended, app not running) and decides what to do with them. Pure; the
//! app feeds observations and applies decisions through `Core`.

use chrono::NaiveTime;
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::model::PauseKind;
use crate::worktime::{local_date, parse_hhmm};
use crate::Result;

const MINUTE_MS: i64 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum AwayAction {
    /// Ask the user; tracking continues until they answer.
    Prompt,
    /// Remove the away time from the entry right away (can be undone).
    Exclude,
    /// Keep it as work time.
    Keep,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase", default)]
pub struct AutoPauseConfig {
    pub enabled: bool,
    /// No input for this long counts as away.
    pub idle_minutes: u32,
    pub idle_action: AwayAction,
    /// Screen locked, system asleep or app not running.
    pub lock_action: AwayAction,
    /// Shorter lock/sleep periods are ignored.
    pub min_away_minutes: u32,
    /// Away periods in the lunch window are excluded as lunch break.
    pub lunch_detection: bool,
    pub lunch_start: String,
    pub lunch_end: String,
    pub lunch_min_minutes: u32,
    /// A timer left running over this time (or overnight) is stopped at the
    /// last activity. `None` disables it.
    pub night_stop: Option<String>,
    /// Notify when the legal daily maximum is exceeded.
    pub warn_daily_max: bool,
    /// Offer to start tracking at the first activity of the day.
    pub morning_prompt: bool,
}

impl Default for AutoPauseConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            idle_minutes: 10,
            idle_action: AwayAction::Prompt,
            lock_action: AwayAction::Exclude,
            min_away_minutes: 2,
            lunch_detection: true,
            lunch_start: "11:30".into(),
            lunch_end: "13:30".into(),
            lunch_min_minutes: 15,
            night_stop: Some("22:00".into()),
            warn_daily_max: true,
            morning_prompt: true,
        }
    }
}

impl AutoPauseConfig {
    pub fn validate(&self) -> Result<()> {
        parse_hhmm(&self.lunch_start)?;
        parse_hhmm(&self.lunch_end)?;
        if let Some(night) = &self.night_stop {
            parse_hhmm(night)?;
        }
        if self.idle_minutes == 0 {
            return Err(crate::Error::validation("idleMinutes", "must be at least 1"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum AwayCause {
    Idle,
    Locked,
    Suspended,
    AppClosed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AwayPeriod {
    pub from: i64,
    pub to: i64,
    pub cause: AwayCause,
}

/// One poll of the OS activity state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observation {
    pub now: i64,
    /// Time since the last input, if the platform can tell.
    pub idle_ms: Option<i64>,
    /// Screen lock state, if the platform can tell.
    pub locked: Option<bool>,
}

/// Turns a stream of observations into away periods. A period is reported
/// once the user is back.
#[derive(Debug, Clone)]
pub struct Detector {
    idle_threshold_ms: i64,
    /// A jump in wall-clock time larger than this between two polls means
    /// the system was asleep.
    suspend_gap_ms: i64,
    away: Option<(i64, AwayCause)>,
    last_now: Option<i64>,
}

impl Detector {
    pub fn new(idle_minutes: u32, poll_interval_ms: i64) -> Self {
        Self {
            idle_threshold_ms: idle_minutes as i64 * MINUTE_MS,
            suspend_gap_ms: (poll_interval_ms * 6).max(30_000),
            away: None,
            last_now: None,
        }
    }

    pub fn set_idle_minutes(&mut self, minutes: u32) {
        self.idle_threshold_ms = minutes as i64 * MINUTE_MS;
    }

    pub fn is_away(&self) -> bool {
        self.away.is_some()
    }

    pub fn feed(&mut self, obs: Observation) -> Option<AwayPeriod> {
        let previous = self.last_now.replace(obs.now);
        let idle = obs.idle_ms.unwrap_or(0).max(0);
        let back_at = obs.now - idle;

        // The process did not run between two polls: the system slept.
        if let Some(prev) = previous {
            if obs.now - prev > self.suspend_gap_ms {
                let from = self.away.take().map_or(prev, |(since, _)| since.min(prev));
                return Some(AwayPeriod {
                    from,
                    to: back_at.max(prev),
                    cause: AwayCause::Suspended,
                });
            }
        }

        let locked = obs.locked == Some(true);
        let idle_away = idle >= self.idle_threshold_ms;
        if locked || idle_away {
            let cause = if locked {
                AwayCause::Locked
            } else {
                AwayCause::Idle
            };
            match &mut self.away {
                Some((since, c)) => {
                    *since = (*since).min(back_at);
                    if locked {
                        *c = AwayCause::Locked;
                    }
                }
                None => self.away = Some((back_at, cause)),
            }
            return None;
        }
        self.away.take().map(|(since, cause)| AwayPeriod {
            from: since,
            to: back_at.max(since),
            cause,
        })
    }
}

/// What to do with an away period while a timer runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Ignore,
    /// Record the period and ask the user.
    Prompt(PauseKind),
    /// Cut the period out of the running entry.
    Exclude(PauseKind),
    /// Stop the running entry at the start of the period.
    StopAtStart,
}

fn minutes_of_day(ms: i64, tz: Tz) -> u32 {
    use chrono::{TimeZone, Timelike};
    let t = tz.timestamp_millis_opt(ms).single().expect("valid timestamp");
    t.hour() * 60 + t.minute()
}

fn hhmm_minutes(time: &str) -> Option<u32> {
    use chrono::Timelike;
    parse_hhmm(time)
        .ok()
        .map(|t: NaiveTime| t.hour() * 60 + t.minute())
}

pub fn decide(period: AwayPeriod, cfg: &AutoPauseConfig, tz: Tz) -> Decision {
    if !cfg.enabled || period.to <= period.from {
        return Decision::Ignore;
    }
    let duration = period.to - period.from;

    // Left running overnight or past the night stop time.
    if let Some(night) = cfg.night_stop.as_deref().and_then(hhmm_minutes) {
        let crosses_midnight = local_date(period.from, tz) != local_date(period.to, tz);
        let from_m = minutes_of_day(period.from, tz);
        let to_m = minutes_of_day(period.to, tz);
        if crosses_midnight || (from_m < night && to_m >= night) {
            return Decision::StopAtStart;
        }
    }

    if cfg.lunch_detection && duration >= cfg.lunch_min_minutes as i64 * MINUTE_MS {
        if let (Some(start), Some(end)) = (hhmm_minutes(&cfg.lunch_start), hhmm_minutes(&cfg.lunch_end)) {
            let from_m = minutes_of_day(period.from, tz);
            let to_m = minutes_of_day(period.to, tz);
            if from_m < end && to_m > start {
                return Decision::Exclude(PauseKind::AutoLunch);
            }
        }
    }

    let (action, kind, min_ms) = match period.cause {
        AwayCause::Idle => (
            cfg.idle_action,
            PauseKind::AutoIdle,
            cfg.idle_minutes as i64 * MINUTE_MS,
        ),
        AwayCause::Locked | AwayCause::Suspended | AwayCause::AppClosed => (
            cfg.lock_action,
            PauseKind::AutoLock,
            cfg.min_away_minutes as i64 * MINUTE_MS,
        ),
    };
    if duration < min_ms {
        return Decision::Ignore;
    }
    match action {
        AwayAction::Prompt => Decision::Prompt(kind),
        AwayAction::Exclude => Decision::Exclude(kind),
        AwayAction::Keep => Decision::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    const VIENNA: Tz = chrono_tz::Europe::Vienna;
    const POLL: i64 = 5_000;

    fn at(day: &str, hhmm: &str) -> i64 {
        let local = day
            .parse::<chrono::NaiveDate>()
            .unwrap()
            .and_time(parse_hhmm(hhmm).unwrap());
        VIENNA
            .from_local_datetime(&local)
            .single()
            .unwrap()
            .timestamp_millis()
    }

    fn obs(now: i64, idle_ms: i64, locked: bool) -> Observation {
        Observation {
            now,
            idle_ms: Some(idle_ms),
            locked: Some(locked),
        }
    }

    #[test]
    fn idle_period_is_reported_when_the_user_returns() {
        let mut d = Detector::new(10, POLL);
        let t0 = at("2026-06-01", "09:00");
        assert_eq!(d.feed(obs(t0, 0, false)), None);
        // 11 minutes without input: away since 09:00:05 ... still away.
        let mut now = t0;
        for _ in 0..140 {
            now += POLL;
            assert_eq!(d.feed(obs(now, now - t0, false)), None);
        }
        assert!(d.is_away());
        // Input again (idle 2 s).
        now += POLL;
        let period = d.feed(obs(now, 2_000, false)).unwrap();
        assert_eq!(period.cause, AwayCause::Idle);
        assert_eq!(period.from, t0);
        assert_eq!(period.to, now - 2_000);
        assert!(!d.is_away());
    }

    #[test]
    fn short_idle_is_not_away() {
        let mut d = Detector::new(10, POLL);
        let t0 = at("2026-06-01", "09:00");
        let mut now = t0;
        while now < t0 + 5 * MINUTE_MS {
            assert_eq!(d.feed(obs(now, now - t0, false)), None);
            now += POLL;
        }
        assert_eq!(d.feed(obs(now, 0, false)), None);
    }

    #[test]
    fn lock_starts_at_last_input() {
        let mut d = Detector::new(10, POLL);
        let t0 = at("2026-06-01", "09:00");
        d.feed(obs(t0, 0, false));
        d.feed(obs(t0 + POLL, 3_000, true)); // locked, last input 3 s ago
        d.feed(obs(t0 + 2 * POLL, POLL + 3_000, true));
        let p = d.feed(obs(t0 + 3 * POLL, 1_000, false)).unwrap();
        assert_eq!(p.cause, AwayCause::Locked);
        assert_eq!(p.from, t0 + POLL - 3_000);
    }

    #[test]
    fn clock_jump_means_suspend() {
        let mut d = Detector::new(10, POLL);
        let t0 = at("2026-06-01", "12:00");
        d.feed(obs(t0, 0, false));
        let p = d.feed(obs(t0 + 45 * MINUTE_MS, 0, false)).unwrap();
        assert_eq!(
            p,
            AwayPeriod {
                from: t0,
                to: t0 + 45 * MINUTE_MS,
                cause: AwayCause::Suspended
            }
        );
    }

    fn period(from: (&str, &str), to: (&str, &str), cause: AwayCause) -> AwayPeriod {
        AwayPeriod {
            from: at(from.0, from.1),
            to: at(to.0, to.1),
            cause,
        }
    }

    #[test]
    fn decisions() {
        let cfg = AutoPauseConfig::default();
        let d = |p| decide(p, &cfg, VIENNA);
        let day = "2026-06-01";
        assert_eq!(
            d(period((day, "09:00"), (day, "09:15"), AwayCause::Idle)),
            Decision::Prompt(PauseKind::AutoIdle)
        );
        assert_eq!(
            d(period((day, "09:00"), (day, "09:05"), AwayCause::Idle)),
            Decision::Ignore
        );
        assert_eq!(
            d(period((day, "09:00"), (day, "09:05"), AwayCause::Locked)),
            Decision::Exclude(PauseKind::AutoLock)
        );
        assert_eq!(
            d(period((day, "09:00"), (day, "09:01"), AwayCause::Locked)),
            Decision::Ignore
        );
        assert_eq!(
            d(period((day, "12:00"), (day, "12:40"), AwayCause::Idle)),
            Decision::Exclude(PauseKind::AutoLunch)
        );
        assert_eq!(
            d(period(
                (day, "18:30"),
                ("2026-06-02", "08:00"),
                AwayCause::Suspended
            )),
            Decision::StopAtStart
        );
        assert_eq!(
            d(period((day, "21:30"), (day, "22:10"), AwayCause::Idle)),
            Decision::StopAtStart
        );
        assert_eq!(
            d(period((day, "22:30"), (day, "22:50"), AwayCause::Idle)),
            Decision::Prompt(PauseKind::AutoIdle)
        );

        let off = AutoPauseConfig {
            enabled: false,
            ..AutoPauseConfig::default()
        };
        assert_eq!(
            decide(
                period((day, "09:00"), (day, "11:00"), AwayCause::Idle),
                &off,
                VIENNA
            ),
            Decision::Ignore
        );
        let keep = AutoPauseConfig {
            lock_action: AwayAction::Keep,
            ..AutoPauseConfig::default()
        };
        assert_eq!(
            decide(
                period((day, "09:00"), (day, "09:30"), AwayCause::Locked),
                &keep,
                VIENNA
            ),
            Decision::Ignore
        );
    }
}
