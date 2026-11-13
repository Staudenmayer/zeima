//! `zeima-core` for JavaScript. Every function takes and returns JSON text,
//! so the server needs no generated bindings: `JSON.parse(holidays(...))`.
//! Errors are thrown as strings.
//!
//! Built with `packages/core-wasm/build.mjs` into the `@zeima/core-wasm`
//! package. The server uses it for work-time, balance, holiday and country
//! profile logic, so it cannot drift from the desktop app (SPEC §3.1).

use std::collections::HashMap;

use chrono::NaiveDate;
use serde::Deserialize;
use wasm_bindgen::prelude::*;
use zeima_core::jurisdiction;
use zeima_core::worktime::{self, AbsenceOnDay, Inputs, WorkSchedule};

fn json<T: serde::Serialize>(value: &T) -> Result<String, JsError> {
    serde_json::to_string(value).map_err(|e| JsError::new(&e.to_string()))
}

fn err(error: impl ToString) -> JsError {
    JsError::new(&error.to_string())
}

/// The bundled country profiles: `[{ id, version, name, defaultLocale, regions }]`.
#[wasm_bindgen]
pub fn profiles() -> Result<String, JsError> {
    json(&jurisdiction::list())
}

/// The profile id for an operating-system locale such as `de-AT`, or `null`.
#[wasm_bindgen(js_name = suggestProfile)]
pub fn suggest_profile(locale: &str) -> Result<String, JsError> {
    json(&jurisdiction::suggest_for_locale(locale))
}

/// Public holidays of a year: `[{ id, date, name, dayOff }]`.
#[wasm_bindgen]
pub fn holidays(profile: &str, region: Option<String>, year: i32) -> Result<String, JsError> {
    let profile = jurisdiction::load(profile, region.as_deref()).map_err(err)?;
    json(&profile.holidays(year))
}

/// The break rule that applies to a day with this many worked minutes, or `null`.
#[wasm_bindgen(js_name = requiredBreak)]
pub fn required_break(profile: &str, region: Option<String>, worked_minutes: u32) -> Result<String, JsError> {
    let profile = jurisdiction::load(profile, region.as_deref()).map_err(err)?;
    json(&profile.required_break(worked_minutes))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AbsenceIn {
    type_id: String,
    fraction: f64,
    reduces_target: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DaysRequest {
    from: NaiveDate,
    to: NaiveDate,
    /// IANA zone, e.g. `Europe/Vienna`.
    tz: String,
    profile: String,
    region: Option<String>,
    schedules: Vec<WorkSchedule>,
    /// `[startMs, endMs]` pairs of finished work.
    intervals: Vec<(i64, i64)>,
    /// Day-off holidays by date, with their names (profile and company).
    #[serde(default)]
    holidays: HashMap<NaiveDate, String>,
    #[serde(default)]
    absences: HashMap<NaiveDate, Vec<AbsenceIn>>,
    /// Balance corrections in minutes by date.
    #[serde(default)]
    adjustments: HashMap<NaiveDate, i64>,
    #[serde(default)]
    deduct_breaks: bool,
    /// End of the last work before `from`, for the rest-time check.
    prev_last_end: Option<i64>,
}

/// Work-time results for `from..=to` (the request shape is `DaysRequest`:
/// zone, profile, schedules, finished work intervals, holidays, absences,
/// corrections). Returns the days and the balance in milliseconds:
/// `{ days: [...], balanceMs }`.
#[wasm_bindgen(js_name = computeDays)]
pub fn compute_days(request: &str) -> Result<String, JsError> {
    let request: DaysRequest = serde_json::from_str(request).map_err(err)?;
    let tz: chrono_tz::Tz = request
        .tz
        .parse()
        .map_err(|_| err(format!("unknown time zone {}", request.tz)))?;
    let profile = jurisdiction::load(&request.profile, request.region.as_deref()).map_err(err)?;
    let absences: HashMap<NaiveDate, Vec<AbsenceOnDay>> = request
        .absences
        .into_iter()
        .map(|(date, list)| {
            let list = list
                .into_iter()
                .map(|a| AbsenceOnDay {
                    type_id: a.type_id,
                    fraction: a.fraction,
                    reduces_target: a.reduces_target,
                })
                .collect();
            (date, list)
        })
        .collect();
    for schedule in &request.schedules {
        schedule.validate().map_err(err)?;
    }
    let inputs = Inputs {
        tz,
        profile: &profile,
        schedules: &request.schedules,
        intervals: &request.intervals,
        holidays: &request.holidays,
        absences: &absences,
        adjustments: &request.adjustments,
        deduct_breaks: request.deduct_breaks,
    };
    let days = worktime::compute_days(request.from, request.to, &inputs, request.prev_last_end);
    let balance_ms = worktime::balance_ms(&days);
    json(&serde_json::json!({ "days": days, "balanceMs": balance_ms }))
}

/// The balance of days returned by `computeDays` (`WorkDay[]` as JSON), in milliseconds.
#[wasm_bindgen(js_name = balanceMs)]
pub fn balance_ms(days: &str) -> Result<f64, JsError> {
    let days: Vec<worktime::WorkDay> = serde_json::from_str(days).map_err(err)?;
    Ok(worktime::balance_ms(&days) as f64)
}
