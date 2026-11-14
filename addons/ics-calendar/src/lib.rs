//! ICS calendar addon: `schedule.tick` downloads the feed and keeps the
//! parsed events in the addon's storage; `timeline.events` expands them for
//! the days the timesheet shows. The parsing is in [`ics`].

pub mod ics;

use ics::{occurrences, overridden_starts, to_ms, Ev};
use zeima_addon_sdk::{Range, TimelineEvent};

/// Storage key of the parsed events.
pub const KEY: &str = "events";
/// Values may be 1 MB; stay well below.
const MAX_STORED: usize = 900_000;
/// Events that ended this long ago and do not repeat are dropped.
const KEEP_DAYS: i64 = 400;

/// What the tick stores: events worth keeping, as JSON.
pub fn build_store(body: &str, now_ms: i64) -> Result<String, String> {
    if !body.contains("BEGIN:VCALENDAR") {
        return Err("The address did not return a calendar (no BEGIN:VCALENDAR). Check the link.".into());
    }
    let cutoff = chrono::DateTime::from_timestamp_millis(now_ms)
        .map(|d| d.naive_utc() - chrono::Duration::days(KEEP_DAYS))
        .unwrap_or(chrono::NaiveDateTime::MIN);
    let mut events: Vec<Ev> = ics::parse(body)
        .into_iter()
        .filter(|e| e.rrule.is_some() || e.rid.is_some() || e.end + chrono::Duration::days(1) >= cutoff)
        .collect();
    events.sort_by_key(|e| e.start);
    // Drop the oldest until it fits.
    loop {
        let json = serde_json::to_string(&events).map_err(|e| e.to_string())?;
        if json.len() <= MAX_STORED || events.is_empty() {
            return Ok(json);
        }
        let drop = (events.len() / 10).max(1);
        events.drain(..drop);
    }
}

/// Timeline items of the stored events in `range`.
pub fn timeline(
    stored: &str,
    range: &Range,
    user_tz: &str,
    hide_cancelled: bool,
    hide_free: bool,
) -> Vec<TimelineEvent> {
    let events: Vec<Ev> = serde_json::from_str(stored).unwrap_or_default();
    let mut out = Vec::new();
    for ev in &events {
        if (hide_cancelled && ev.cancelled) || (hide_free && ev.free) {
            continue;
        }
        let overridden = if ev.rid.is_none() && ev.rrule.is_some() {
            overridden_starts(&events, &ev.uid, user_tz)
        } else {
            Vec::new()
        };
        let recurring = ev.rrule.is_some();
        // The changed instance of a recurring event is a plain event.
        let single = Ev {
            rrule: None,
            ..ev.clone()
        };
        let source = if ev.rid.is_some() { &single } else { ev };
        for occ in occurrences(source, range.from, range.to, user_tz, &overridden) {
            let start_at = to_ms(occ.start, ev.tz.as_deref(), user_tz);
            let end_at = to_ms(occ.end, ev.tz.as_deref(), user_tz);
            if !ev.all_day && end_at <= start_at {
                continue;
            }
            out.push(TimelineEvent {
                id: if recurring || ev.rid.is_some() {
                    format!("{}@{}", ev.uid, start_at)
                } else {
                    ev.uid.clone()
                },
                title: ev.title.clone(),
                start_at,
                end_at,
                all_day: ev.all_day,
                detail: ev.location.clone(),
                ..Default::default()
            });
        }
    }
    out.sort_by_key(|e| (e.start_at, e.id.clone()));
    out
}

/// `webcal://` and `webcals://` are the same address over https.
pub fn feed_url(address: &str) -> String {
    let a = address.trim();
    for prefix in ["webcals://", "webcal://", "WEBCAL://"] {
        if let Some(rest) = a.strip_prefix(prefix) {
            return format!("https://{rest}");
        }
    }
    a.to_string()
}

#[cfg(target_arch = "wasm32")]
mod plugin {
    use extism_pdk::{config, http, plugin_fn, FnResult, HttpRequest, Json};
    use zeima_addon_sdk::{host, Tick, TimelineEvent, TimelineRequest};

    fn setting(key: &str) -> String {
        config::get(key).ok().flatten().unwrap_or_default()
    }

    #[plugin_fn]
    pub fn on_schedule_tick(Json(tick): Json<Tick>) -> FnResult<()> {
        let address = setting("url");
        if address.trim().is_empty() {
            return Err(extism_pdk::Error::msg("Enter the calendar address in the addon's settings.").into());
        }
        let url = crate::feed_url(&address);
        if !url.starts_with("https://") && !url.starts_with("http://") {
            return Err(extism_pdk::Error::msg("The address must start with https:// or webcal://.").into());
        }
        let res = http::request::<()>(
            &HttpRequest::new(&url).with_header("Accept", "text/calendar, */*"),
            None,
        )?;
        let status = res.status_code();
        if !(200..300).contains(&status) {
            return Err(extism_pdk::Error::msg(format!(
                "The calendar server answered {status}. Check the link."
            ))
            .into());
        }
        let body = String::from_utf8_lossy(&res.body()).into_owned();
        let stored = crate::build_store(&body, tick.now).map_err(extism_pdk::Error::msg)?;
        host::kv_set(crate::KEY, &stored)?;
        Ok(())
    }

    #[plugin_fn]
    pub fn timeline_events(Json(req): Json<TimelineRequest>) -> FnResult<Json<Vec<TimelineEvent>>> {
        let Some(stored) = host::kv_get(crate::KEY)? else {
            return Ok(Json(Vec::new()));
        };
        let flag = |key: &str, default: bool| match setting(key).as_str() {
            "true" => true,
            "false" => false,
            _ => default,
        };
        Ok(Json(crate::timeline(
            &stored,
            &req.range,
            &req.timezone,
            flag("hide_cancelled", true),
            flag("hide_free", false),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FEED: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\n\
BEGIN:VEVENT\r\nUID:one\r\nSUMMARY:Review\r\nLOCATION:Room 2\r\nDTSTART;TZID=Europe/Vienna:20261105T090000\r\nDTEND;TZID=Europe/Vienna:20261105T100000\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:wk\r\nSUMMARY:Standup\r\nDTSTART;TZID=Europe/Vienna:20261102T090000\r\nDTEND;TZID=Europe/Vienna:20261102T091500\r\nRRULE:FREQ=WEEKLY;BYDAY=MO\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:wk\r\nSUMMARY:Standup (late)\r\nRECURRENCE-ID;TZID=Europe/Vienna:20261109T090000\r\nDTSTART;TZID=Europe/Vienna:20261109T100000\r\nDTEND;TZID=Europe/Vienna:20261109T101500\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:cx\r\nSUMMARY:Cancelled\r\nSTATUS:CANCELLED\r\nDTSTART:20261105T120000Z\r\nDTEND:20261105T130000Z\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:hol\r\nSUMMARY:Holiday\r\nDTSTART;VALUE=DATE:20261105\r\nDTEND;VALUE=DATE:20261106\r\nEND:VEVENT\r\n\
END:VCALENDAR\r\n";

    fn ms(y: i32, m: u32, d: u32) -> i64 {
        use chrono::NaiveDate;
        to_ms(
            NaiveDate::from_ymd_opt(y, m, d)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            None,
            "Europe/Vienna",
        )
    }

    #[test]
    fn stores_and_expands_for_the_week() {
        let now = ms(2026, 11, 1);
        let stored = build_store(FEED, now).unwrap();
        let range = Range {
            from: ms(2026, 11, 2),
            to: ms(2026, 11, 16),
        };
        let items = timeline(&stored, &range, "Europe/Vienna", true, false);
        let titles: Vec<&str> = items.iter().map(|e| e.title.as_str()).collect();
        // Monday 2 standup; on 5 Nov the all-day holiday (midnight) and the review; the moved
        // standup on 9; the cancelled event is hidden and the 16th is outside.
        assert_eq!(titles, vec!["Standup", "Holiday", "Review", "Standup (late)"]);
        let review = items.iter().find(|e| e.title == "Review").unwrap();
        assert_eq!(review.detail.as_deref(), Some("Room 2"));
        assert_eq!(review.end_at - review.start_at, 3_600_000);
        let holiday = items.iter().find(|e| e.title == "Holiday").unwrap();
        assert!(holiday.all_day);
        assert_eq!(holiday.start_at, ms(2026, 11, 5));
        // Ids are unique per instance.
        let mut ids: Vec<_> = items.iter().map(|e| e.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), items.len());
        // Cancelled events can be shown.
        assert_eq!(timeline(&stored, &range, "Europe/Vienna", false, false).len(), 5);
    }

    #[test]
    fn rejects_other_content_and_old_events_are_dropped() {
        assert!(build_store("<html>login</html>", 0).is_err());
        let old = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:o\r\nSUMMARY:old\r\nDTSTART:20200101T100000Z\r\nDTEND:20200101T110000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let stored = build_store(old, ms(2026, 11, 1)).unwrap();
        assert_eq!(stored, "[]");
    }

    #[test]
    fn webcal_links_become_https() {
        assert_eq!(
            feed_url(" webcal://cal.example.com/a.ics "),
            "https://cal.example.com/a.ics"
        );
        assert_eq!(feed_url("https://x.org/a.ics"), "https://x.org/a.ics");
    }
}
