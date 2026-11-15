//! Day digest: once a day, from the hour the user chose, a notification says
//! what the day holds: the meetings they take part in, an absence, the target.
//! It also reacts to `meeting.changed` (a cancelled meeting) and
//! `absence.changed` (an absence request that was decided).
//!
//! Shows `host::list_meetings`, `list_absences`, `list_work_days` and
//! `list_activities`, the local date and time of a [`Tick`], and the newer events.

use extism_pdk::{config, plugin_fn, FnResult, Json};
use zeima_addon_sdk::{host, DateRange, Event, MeetingOccurrence, Range, Tick};

/// `kv` key: the date the digest was last sent, so it comes once a day.
const LAST_DIGEST: &str = "last_digest";
const HOUR: i64 = 3_600_000;
const DAY: i64 = 24 * HOUR;

fn flag(key: &str) -> bool {
    config::get(key).ok().flatten().as_deref() != Some("false")
}

/// `08:30` as minutes after midnight.
fn minutes(clock: &str) -> Option<u32> {
    let (h, m) = clock.split_once(':')?;
    Some(h.parse::<u32>().ok()? * 60 + m.parse::<u32>().ok()?)
}

/// Whether the person takes part: they organize it, or they did not decline it.
fn takes_part(o: &MeetingOccurrence) -> bool {
    !o.meeting.cancelled && (o.is_organizer || o.my_status.as_deref() != Some("declined"))
}

/// `1:45 h` from milliseconds.
fn hours(ms: i64) -> String {
    let m = ms.max(0) / 60_000;
    format!("{}:{:02} h", m / 60, m % 60)
}

#[plugin_fn]
pub fn on_schedule_tick(Json(tick): Json<Tick>) -> FnResult<()> {
    let from_hour: u32 = config::get("hour")?.and_then(|v| v.parse().ok()).unwrap_or(8).min(23);
    let Some(now) = minutes(&tick.local_time) else { return Ok(()) };
    if now < from_hour * 60 || host::kv_get(LAST_DIGEST)?.as_deref() == Some(tick.local_date.as_str()) {
        return Ok(());
    }
    // One day: from the start of the date to its end, in the user's zone. The addon has no
    // time zone database, so it takes the day as the 24 hours around now, which is enough to
    // list what is left of it.
    let start = tick.now - i64::from(now) * 60_000;
    let day = DateRange {
        from: tick.local_date.clone(),
        to: tick.local_date.clone(),
    };
    let Some(work) = host::list_work_days(day.clone())?.into_iter().next() else {
        return Ok(());
    };
    // A public holiday: no digest.
    if work.target_ms == 0 && work.holiday.is_some() {
        host::kv_set(LAST_DIGEST, &tick.local_date)?;
        return Ok(());
    }
    let activities = host::list_activities()?;
    let mut lines: Vec<String> = Vec::new();

    // Meetings of the day the person takes part in, with the activity they will be tracked under.
    let mut meetings = host::list_meetings(Range {
        from: start,
        to: start + DAY,
    })?;
    meetings.retain(|o| o.start_at >= start && o.start_at < start + DAY);
    meetings.sort_by_key(|o| o.start_at);
    let mine: Vec<_> = meetings.iter().filter(|o| takes_part(o)).collect();
    if let Some(first) = mine.first() {
        let activity = first
            .meeting
            .activity_id
            .as_ref()
            .and_then(|id| activities.iter().find(|a| &a.id == id))
            .map(|a| format!(" ({})", a.name))
            .unwrap_or_default();
        lines.push(format!(
            "{} meeting(s), the first: {}{activity}",
            mine.len(),
            first.meeting.title
        ));
    }
    // An absence (also a half day).
    for a in host::list_absences(day)? {
        if a.status != "rejected" {
            let half = a.half_day.map(|h| format!(" ({h})")).unwrap_or_default();
            lines.push(format!("{}{half}", a.type_name));
        }
    }
    if work.target_ms > 0 {
        lines.push(format!("target {}", hours(work.target_ms)));
    }
    if let Some(name) = &work.holiday {
        lines.push(name.clone());
    }
    if !lines.is_empty() {
        host::notify("Today", &lines.join(" · "))?;
    }
    host::kv_set(LAST_DIGEST, &tick.local_date)?;
    Ok(())
}

#[plugin_fn]
pub fn on_event(Json(event): Json<Event>) -> FnResult<()> {
    match (event.kind.as_str(), event.change().as_deref()) {
        ("meeting.changed", Some("cancelled")) if flag("cancelled") => {
            if let Some(m) = event.meeting() {
                host::notify("Meeting cancelled", &m.title)?;
            }
        }
        ("absence.changed", Some(change @ ("approved" | "rejected"))) if flag("decisions") => {
            if let Some(a) = event.absence() {
                let note = if a.decision_note.is_empty() {
                    String::new()
                } else {
                    format!(": {}", a.decision_note)
                };
                host::notify(
                    "Absence request",
                    &format!("{} to {} was {change}{note}", a.start_date, a.end_date),
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}
