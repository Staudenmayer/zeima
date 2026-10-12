//! Work-time calculation: worked time, breaks, targets, balance and legal
//! warnings per local day. Pure functions; the db layer feeds them.

use std::collections::HashMap;

use chrono::{Datelike, Duration, NaiveDate, NaiveTime, TimeZone};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::jurisdiction::Profile;
use crate::summary::union_ms;
use crate::{Error, Result};

const MINUTE_MS: i64 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum ScheduleModel {
    /// Fixed hours; deviations count towards the balance.
    Fixed,
    /// Flexitime around core hours; deviations count towards the balance.
    Flextime,
    /// Trust-based: targets are shown, but no balance is kept.
    Trust,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct DayPlan {
    pub target_minutes: u32,
    /// Core hours `HH:MM` (flextime). Informational for now.
    pub core_start: Option<String>,
    pub core_end: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct WorkSchedule {
    pub id: String,
    /// Applies from this date until the next schedule's `valid_from`.
    pub valid_from: NaiveDate,
    pub model: ScheduleModel,
    /// Monday first.
    pub days: Vec<DayPlan>,
}

impl WorkSchedule {
    pub fn plan(&self, date: NaiveDate) -> Option<&DayPlan> {
        self.days.get(date.weekday().num_days_from_monday() as usize)
    }

    pub fn validate(&self) -> Result<()> {
        if self.days.len() != 7 {
            return Err(Error::validation("days", "a schedule needs 7 days"));
        }
        for day in &self.days {
            if day.target_minutes > 24 * 60 {
                return Err(Error::validation("days", "a day cannot have more than 24 h"));
            }
            for time in [&day.core_start, &day.core_end].into_iter().flatten() {
                parse_hhmm(time)?;
            }
        }
        Ok(())
    }
}

/// The schedule in effect on `date` (latest `valid_from` ≤ date).
pub fn schedule_on(schedules: &[WorkSchedule], date: NaiveDate) -> Option<&WorkSchedule> {
    schedules
        .iter()
        .filter(|s| s.valid_from <= date)
        .max_by_key(|s| s.valid_from)
}

pub fn parse_hhmm(value: &str) -> Result<NaiveTime> {
    NaiveTime::parse_from_str(value, "%H:%M")
        .map_err(|_| Error::validation("time", format!("'{value}' is not a time like 09:30")))
}

/// An absence covering (part of) a day.
#[derive(Debug, Clone, PartialEq)]
pub struct AbsenceOnDay {
    pub type_id: String,
    /// 1.0 for a full day, 0.5 for a half day.
    pub fraction: f64,
    /// Vacation, sickness etc. reduce the target; comp time does not (it
    /// is taken from the balance).
    pub reduces_target: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DayWarning {
    /// Worked more than the legal daily maximum.
    ExceedsDailyMax {
        #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
        max_minutes: i64,
    },
    /// Rest since the previous working day was shorter than required.
    RestTooShort {
        #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
        rest_minutes: i64,
        #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
        min_minutes: i64,
    },
    /// Required break not taken (only when breaks are not deducted).
    BreakTooShort {
        #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
        required_minutes: i64,
        #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
        taken_minutes: i64,
    },
}

/// Result for one local day. All durations in ms.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct WorkDay {
    pub date: NaiveDate,
    /// Time covered by entries (overlaps once).
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub worked_ms: i64,
    /// Gaps between entries that count as break.
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub break_taken_ms: i64,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub break_required_ms: i64,
    /// Missing break deducted from the worked time.
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub break_deducted_ms: i64,
    /// Worked time after deductions; what counts.
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub net_ms: i64,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub target_ms: i64,
    /// `net - target` (0 for trust-based schedules).
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub delta_ms: i64,
    /// Manual balance corrections on this day.
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub adjustment_ms: i64,
    #[cfg_attr(feature = "specta", specta(type = Option<specta_typescript::Number>))]
    pub first_start: Option<i64>,
    #[cfg_attr(feature = "specta", specta(type = Option<specta_typescript::Number>))]
    pub last_end: Option<i64>,
    /// Name of the holiday if the day is a public holiday (day off).
    pub holiday: Option<String>,
    pub absence_type_ids: Vec<String>,
    pub warnings: Vec<DayWarning>,
}

/// Start and end (unix ms) of a local calendar day. Handles DST (23/25 h
/// days) and zones where midnight does not exist on some days.
pub fn day_bounds(date: NaiveDate, tz: Tz) -> (i64, i64) {
    let start_of = |d: NaiveDate| -> i64 {
        let midnight = d.and_hms_opt(0, 0, 0).expect("midnight");
        match tz.from_local_datetime(&midnight).earliest() {
            Some(t) => t.timestamp_millis(),
            // Midnight skipped by a DST jump: the day starts at the first valid instant.
            None => tz
                .from_local_datetime(&(midnight + Duration::hours(1)))
                .earliest()
                .map(|t| t.timestamp_millis())
                .expect("valid local time after DST gap"),
        }
    };
    (start_of(date), start_of(date + Duration::days(1)))
}

/// Local date of an instant.
pub fn local_date(ms: i64, tz: Tz) -> NaiveDate {
    tz.timestamp_millis_opt(ms)
        .single()
        .map(|t| t.date_naive())
        .expect("valid timestamp")
}

/// Everything needed to compute days. Entries are `(start, end)` with
/// running entries already closed at "now".
pub struct Inputs<'a> {
    pub tz: Tz,
    pub profile: &'a Profile,
    pub schedules: &'a [WorkSchedule],
    pub intervals: &'a [(i64, i64)],
    /// Day-off holidays by date (profile + custom), with display name.
    pub holidays: &'a HashMap<NaiveDate, String>,
    pub absences: &'a HashMap<NaiveDate, Vec<AbsenceOnDay>>,
    /// Balance corrections in minutes by date.
    pub adjustments: &'a HashMap<NaiveDate, i64>,
    /// Deduct missing legal breaks from the worked time.
    pub deduct_breaks: bool,
}

/// Merged, sorted intervals clipped to `[from, to)`.
fn clipped(intervals: &[(i64, i64)], from: i64, to: i64) -> Vec<(i64, i64)> {
    let mut parts: Vec<(i64, i64)> = intervals
        .iter()
        .map(|&(s, e)| (s.max(from), e.min(to)))
        .filter(|(s, e)| e > s)
        .collect();
    parts.sort_unstable();
    let mut merged: Vec<(i64, i64)> = Vec::with_capacity(parts.len());
    for (s, e) in parts {
        match merged.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    merged
}

pub fn compute_day(date: NaiveDate, inputs: &Inputs<'_>, prev_last_end: Option<i64>) -> WorkDay {
    let (from, to) = day_bounds(date, inputs.tz);
    let merged = clipped(inputs.intervals, from, to);
    let worked_ms = union_ms(merged.clone());
    let first_start = merged.first().map(|m| m.0);
    let last_end = merged.last().map(|m| m.1);

    // Breaks
    let worked_minutes = (worked_ms / MINUTE_MS) as u32;
    let rule = inputs.profile.required_break(worked_minutes);
    let break_required_ms = rule.map_or(0, |r| r.break_minutes as i64 * MINUTE_MS);
    let min_part_ms = rule.map_or(0, |r| r.min_part_minutes as i64 * MINUTE_MS);
    let break_taken_ms: i64 = merged
        .windows(2)
        .map(|w| w[1].0 - w[0].1)
        .filter(|gap| *gap >= min_part_ms)
        .sum();
    let missing = (break_required_ms - break_taken_ms).max(0);
    let break_deducted_ms = if inputs.deduct_breaks {
        missing.min(worked_ms)
    } else {
        0
    };
    let net_ms = worked_ms - break_deducted_ms;

    // Target
    let holiday = inputs.holidays.get(&date).cloned();
    let absences = inputs.absences.get(&date).cloned().unwrap_or_default();
    let schedule = schedule_on(inputs.schedules, date);
    let mut target_ms = schedule
        .and_then(|s| s.plan(date))
        .map_or(0, |p| p.target_minutes as i64 * MINUTE_MS);
    if holiday.is_some() {
        target_ms = 0;
    }
    let reduced: f64 = absences
        .iter()
        .filter(|a| a.reduces_target)
        .map(|a| a.fraction)
        .sum::<f64>()
        .min(1.0);
    target_ms = (target_ms as f64 * (1.0 - reduced)).round() as i64;
    let delta_ms = match schedule.map(|s| s.model) {
        Some(ScheduleModel::Trust) | None => 0,
        Some(_) => net_ms - target_ms,
    };
    let adjustment_ms = inputs.adjustments.get(&date).copied().unwrap_or(0) * MINUTE_MS;

    // Warnings
    let mut warnings = Vec::new();
    let wt = &inputs.profile.work_time;
    if let Some(max) = wt.max_daily_minutes {
        if net_ms > max as i64 * MINUTE_MS {
            warnings.push(DayWarning::ExceedsDailyMax {
                max_minutes: max as i64,
            });
        }
    }
    if let (Some(min), Some(prev), Some(start)) = (wt.min_daily_rest_minutes, prev_last_end, first_start) {
        let rest = start - prev;
        if rest < min as i64 * MINUTE_MS {
            warnings.push(DayWarning::RestTooShort {
                rest_minutes: rest / MINUTE_MS,
                min_minutes: min as i64,
            });
        }
    }
    if !inputs.deduct_breaks && missing > 0 {
        warnings.push(DayWarning::BreakTooShort {
            required_minutes: break_required_ms / MINUTE_MS,
            taken_minutes: break_taken_ms / MINUTE_MS,
        });
    }

    WorkDay {
        date,
        worked_ms,
        break_taken_ms,
        break_required_ms,
        break_deducted_ms,
        net_ms,
        target_ms,
        delta_ms,
        adjustment_ms,
        first_start,
        last_end,
        holiday,
        absence_type_ids: absences.into_iter().map(|a| a.type_id).collect(),
        warnings,
    }
}

/// Days `from..=to`. `prev_last_end` is the last work end before `from`
/// (for the rest-time check on the first day).
pub fn compute_days(
    from: NaiveDate,
    to: NaiveDate,
    inputs: &Inputs<'_>,
    prev_last_end: Option<i64>,
) -> Vec<WorkDay> {
    let mut days = Vec::new();
    let mut prev = prev_last_end;
    let mut date = from;
    while date <= to {
        let day = compute_day(date, inputs, prev);
        if day.last_end.is_some() {
            prev = day.last_end;
        }
        days.push(day);
        date += Duration::days(1);
    }
    days
}

/// Balance contribution of days: deltas plus adjustments.
pub fn balance_ms(days: &[WorkDay]) -> i64 {
    days.iter().map(|d| d.delta_ms + d.adjustment_ms).sum()
}

#[cfg(test)]
mod tests;
