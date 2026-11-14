//! A small iCalendar reader: events, time zones, all-day events and the
//! common recurrence rules. No I/O, so it is tested natively.
//!
//! Not supported (ignored, the event shows once): `BYSETPOS`, `BYWEEKNO`,
//! `BYYEARDAY`, `RDATE`, rules finer than a day.

use std::collections::HashMap;

use chrono::{Datelike, Duration, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Weekday};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

/// A local date and time and the zone it is in (`None` = floating: the
/// user's own zone; "UTC" for values ending in `Z`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stamp {
    pub at: NaiveDateTime,
    pub tz: Option<String>,
}

/// One VEVENT, as stored between syncs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ev {
    pub uid: String,
    pub title: String,
    pub location: Option<String>,
    pub all_day: bool,
    pub start: NaiveDateTime,
    /// In the zone of `tz`; exclusive for all-day events.
    pub end: NaiveDateTime,
    pub tz: Option<String>,
    pub rrule: Option<String>,
    pub exdates: Vec<Stamp>,
    /// Set on the changed instance of a recurring event (RECURRENCE-ID).
    pub rid: Option<Stamp>,
    pub cancelled: bool,
    /// TRANSP:TRANSPARENT, the event does not block the time.
    pub free: bool,
}

// ---------------------------------------------------------------- parsing

struct Prop {
    name: String,
    params: HashMap<String, String>,
    value: String,
}

fn unfold(src: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in src.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = raw.strip_prefix([' ', '\t']) {
            if let Some(last) = lines.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        lines.push(raw.to_string());
    }
    lines
}

/// Splits at the first `sep` outside double quotes.
fn split_unquoted(s: &str, sep: char) -> Option<(&str, &str)> {
    let mut quoted = false;
    for (i, c) in s.char_indices() {
        if c == '"' {
            quoted = !quoted;
        } else if c == sep && !quoted {
            return Some((&s[..i], &s[i + c.len_utf8()..]));
        }
    }
    None
}

fn parse_prop(line: &str) -> Option<Prop> {
    let (head, value) = split_unquoted(line, ':')?;
    let mut parts = Vec::new();
    let mut rest = head;
    while let Some((a, b)) = split_unquoted(rest, ';') {
        parts.push(a);
        rest = b;
    }
    parts.push(rest);
    let name = parts[0].to_ascii_uppercase();
    let mut params = HashMap::new();
    for p in &parts[1..] {
        if let Some((k, v)) = p.split_once('=') {
            params.insert(k.to_ascii_uppercase(), v.trim_matches('"').to_string());
        }
    }
    Some(Prop {
        name,
        params,
        value: value.to_string(),
    })
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// Windows time zone names, as Outlook writes them, to IANA names.
fn windows_zone(name: &str) -> Option<&'static str> {
    Some(match name {
        "W. Europe Standard Time" => "Europe/Berlin",
        "Central Europe Standard Time" => "Europe/Budapest",
        "Central European Standard Time" => "Europe/Warsaw",
        "Romance Standard Time" => "Europe/Paris",
        "GMT Standard Time" => "Europe/London",
        "Greenwich Standard Time" => "Atlantic/Reykjavik",
        "E. Europe Standard Time" => "Europe/Chisinau",
        "FLE Standard Time" => "Europe/Kiev",
        "GTB Standard Time" => "Europe/Bucharest",
        "Russian Standard Time" => "Europe/Moscow",
        "Turkey Standard Time" => "Europe/Istanbul",
        "Eastern Standard Time" => "America/New_York",
        "Central Standard Time" => "America/Chicago",
        "Mountain Standard Time" => "America/Denver",
        "Pacific Standard Time" => "America/Los_Angeles",
        "US Mountain Standard Time" => "America/Phoenix",
        "Alaskan Standard Time" => "America/Anchorage",
        "Hawaiian Standard Time" => "Pacific/Honolulu",
        "Atlantic Standard Time" => "America/Halifax",
        "Canada Central Standard Time" => "America/Regina",
        "Mexico Standard Time" => "America/Mexico_City",
        "SA Pacific Standard Time" => "America/Bogota",
        "E. South America Standard Time" => "America/Sao_Paulo",
        "Argentina Standard Time" => "America/Argentina/Buenos_Aires",
        "India Standard Time" => "Asia/Kolkata",
        "China Standard Time" => "Asia/Shanghai",
        "Tokyo Standard Time" => "Asia/Tokyo",
        "Korea Standard Time" => "Asia/Seoul",
        "Singapore Standard Time" => "Asia/Singapore",
        "Arabian Standard Time" => "Asia/Dubai",
        "Israel Standard Time" => "Asia/Jerusalem",
        "Egypt Standard Time" => "Africa/Cairo",
        "South Africa Standard Time" => "Africa/Johannesburg",
        "AUS Eastern Standard Time" => "Australia/Sydney",
        "New Zealand Standard Time" => "Pacific/Auckland",
        "UTC" => "UTC",
        _ => return None,
    })
}

fn resolve_zone(name: &str) -> Option<String> {
    let name = name.trim_matches('"');
    if name.parse::<Tz>().is_ok() {
        return Some(name.to_string());
    }
    windows_zone(name).map(str::to_string)
}

/// `20261105T090000Z`, `20261105T090000` or `20261105`.
fn parse_stamp(value: &str, tzid: Option<&str>) -> Option<(Stamp, bool)> {
    let v = value.trim();
    let (body, utc) = match v.strip_suffix(['Z', 'z']) {
        Some(b) => (b, true),
        None => (v, false),
    };
    if body.len() == 8 {
        let d = NaiveDate::parse_from_str(body, "%Y%m%d").ok()?;
        return Some((
            Stamp {
                at: d.and_time(NaiveTime::MIN),
                tz: None,
            },
            true,
        ));
    }
    let at = NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S").ok()?;
    let tz = if utc {
        Some("UTC".to_string())
    } else {
        tzid.and_then(resolve_zone)
    };
    Some((Stamp { at, tz }, false))
}

fn parse_duration(v: &str) -> Option<Duration> {
    let v = v.trim();
    let (neg, v) = match v.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, v.strip_prefix('+').unwrap_or(v)),
    };
    let v = v.strip_prefix('P')?;
    let mut total = Duration::zero();
    let mut num = String::new();
    let mut in_time = false;
    for c in v.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => num.push(c),
            unit => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                total += match (unit, in_time) {
                    ('W', _) => Duration::weeks(n),
                    ('D', _) => Duration::days(n),
                    ('H', true) => Duration::hours(n),
                    ('M', true) => Duration::minutes(n),
                    ('S', true) => Duration::seconds(n),
                    _ => return None,
                };
            }
        }
    }
    Some(if neg { -total } else { total })
}

/// Milliseconds since the epoch of a local time in `tz` (`None` = `user`).
/// Unknown zones count as UTC; a time that does not exist (spring
/// forward) moves one hour on.
pub fn to_ms(at: NaiveDateTime, tz: Option<&str>, user: &str) -> i64 {
    let zone: Tz = tz
        .and_then(|z| z.parse().ok())
        .or_else(|| user.parse().ok())
        .unwrap_or(Tz::UTC);
    match zone.from_local_datetime(&at) {
        LocalResult::Single(d) => d.timestamp_millis(),
        LocalResult::Ambiguous(first, _) => first.timestamp_millis(),
        LocalResult::None => zone
            .from_local_datetime(&(at + Duration::hours(1)))
            .earliest()
            .map(|d| d.timestamp_millis())
            .unwrap_or_else(|| at.and_utc().timestamp_millis()),
    }
}

/// Converts a local time between zones (`None` = the user's).
fn convert(at: NaiveDateTime, from: &Option<String>, to: &Option<String>) -> NaiveDateTime {
    if from == to || from.is_none() || to.is_none() {
        return at;
    }
    let ms = to_ms(at, from.as_deref(), "UTC");
    let utc = chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| d.naive_utc())
        .unwrap_or(at);
    match to.as_deref().and_then(|z| z.parse::<Tz>().ok()) {
        Some(zone) => zone.from_utc_datetime(&utc).naive_local(),
        None => utc,
    }
}

#[derive(Default)]
struct Draft {
    uid: String,
    title: String,
    location: Option<String>,
    start: Option<(Stamp, bool)>,
    end: Option<(Stamp, bool)>,
    duration: Option<Duration>,
    rrule: Option<String>,
    exdates: Vec<Stamp>,
    rid: Option<Stamp>,
    cancelled: bool,
    free: bool,
}

impl Draft {
    fn finish(self) -> Option<Ev> {
        let (start, all_day) = self.start?;
        let end_at = match (self.end, self.duration) {
            (Some((end, _)), _) => convert(end.at, &end.tz, &start.tz),
            (None, Some(d)) => start.at + d,
            (None, None) if all_day => start.at + Duration::days(1),
            (None, None) => start.at,
        };
        Some(Ev {
            uid: if self.uid.is_empty() {
                format!("{}", start.at.and_utc().timestamp())
            } else {
                self.uid
            },
            title: if self.title.trim().is_empty() {
                "(no title)".to_string()
            } else {
                self.title
            },
            location: self.location.filter(|l| !l.trim().is_empty()),
            all_day,
            start: start.at,
            end: end_at,
            tz: start.tz,
            rrule: self.rrule,
            exdates: self.exdates,
            rid: self.rid,
            cancelled: self.cancelled,
            free: self.free,
        })
    }
}

/// Reads all events of an iCalendar file. Broken events are skipped.
pub fn parse(src: &str) -> Vec<Ev> {
    let mut events = Vec::new();
    let mut draft: Option<Draft> = None;
    let mut alarm_depth = 0u32;
    for line in unfold(src) {
        let Some(prop) = parse_prop(&line) else { continue };
        match (prop.name.as_str(), prop.value.as_str()) {
            ("BEGIN", "VEVENT") => {
                draft = Some(Draft::default());
                alarm_depth = 0;
                continue;
            }
            ("END", "VEVENT") => {
                if let Some(ev) = draft.take().and_then(Draft::finish) {
                    events.push(ev);
                }
                continue;
            }
            ("BEGIN", "VALARM") if draft.is_some() => {
                alarm_depth += 1;
                continue;
            }
            ("END", "VALARM") if draft.is_some() => {
                alarm_depth = alarm_depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }
        let Some(d) = draft.as_mut() else { continue };
        if alarm_depth > 0 {
            continue;
        }
        let tzid = prop.params.get("TZID").map(String::as_str);
        match prop.name.as_str() {
            "UID" => d.uid = prop.value.trim().to_string(),
            "SUMMARY" => d.title = unescape(&prop.value),
            "LOCATION" => d.location = Some(unescape(&prop.value)),
            "DTSTART" => d.start = parse_stamp(&prop.value, tzid),
            "DTEND" => d.end = parse_stamp(&prop.value, tzid),
            "DURATION" => d.duration = parse_duration(&prop.value),
            "RRULE" => d.rrule = Some(prop.value.trim().to_string()),
            "EXDATE" => {
                for part in prop.value.split(',') {
                    if let Some((s, _)) = parse_stamp(part, tzid) {
                        d.exdates.push(s);
                    }
                }
            }
            "RECURRENCE-ID" => d.rid = parse_stamp(&prop.value, tzid).map(|(s, _)| s),
            "STATUS" => d.cancelled = prop.value.trim().eq_ignore_ascii_case("CANCELLED"),
            "TRANSP" => d.free = prop.value.trim().eq_ignore_ascii_case("TRANSPARENT"),
            _ => {}
        }
    }
    events
}

// ------------------------------------------------------------- recurrence

#[derive(Debug, Clone, Copy, PartialEq)]
enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

enum Until {
    Local(NaiveDateTime),
    Utc(i64),
}

struct Rule {
    freq: Freq,
    interval: i64,
    count: Option<u32>,
    until: Option<Until>,
    by_day: Vec<(Option<i32>, Weekday)>,
    by_monthday: Vec<i32>,
    by_month: Vec<u32>,
    wkst: Weekday,
}

fn weekday_of(code: &str) -> Option<Weekday> {
    Some(match code {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        _ => return None,
    })
}

fn parse_rule(text: &str) -> Option<Rule> {
    let mut rule = Rule {
        freq: Freq::Daily,
        interval: 1,
        count: None,
        until: None,
        by_day: Vec::new(),
        by_monthday: Vec::new(),
        by_month: Vec::new(),
        wkst: Weekday::Mon,
    };
    let mut have_freq = false;
    for part in text.split(';') {
        let (k, v) = part.split_once('=')?;
        match k.to_ascii_uppercase().as_str() {
            "FREQ" => {
                rule.freq = match v.to_ascii_uppercase().as_str() {
                    "DAILY" => Freq::Daily,
                    "WEEKLY" => Freq::Weekly,
                    "MONTHLY" => Freq::Monthly,
                    "YEARLY" => Freq::Yearly,
                    _ => return None, // hourly and finer: not supported
                };
                have_freq = true;
            }
            "INTERVAL" => rule.interval = v.parse::<i64>().ok()?.max(1),
            "COUNT" => rule.count = v.parse().ok(),
            "UNTIL" => {
                let (stamp, all_day) = parse_stamp(v, None)?;
                rule.until = Some(match (stamp.tz.as_deref(), all_day) {
                    (Some("UTC"), _) => Until::Utc(stamp.at.and_utc().timestamp_millis()),
                    // A date: the whole day counts.
                    (_, true) => Until::Local(
                        stamp.at + Duration::hours(23) + Duration::minutes(59) + Duration::seconds(59),
                    ),
                    _ => Until::Local(stamp.at),
                });
            }
            "BYDAY" => {
                for item in v.split(',') {
                    let item = item.trim().to_ascii_uppercase();
                    if item.len() < 2 {
                        return None;
                    }
                    let (num, code) = item.split_at(item.len() - 2);
                    let ord = if num.is_empty() {
                        None
                    } else {
                        Some(num.parse::<i32>().ok()?)
                    };
                    rule.by_day.push((ord, weekday_of(code)?));
                }
            }
            "BYMONTHDAY" => rule.by_monthday = v.split(',').filter_map(|n| n.trim().parse().ok()).collect(),
            "BYMONTH" => rule.by_month = v.split(',').filter_map(|n| n.trim().parse().ok()).collect(),
            "WKST" => rule.wkst = weekday_of(&v.to_ascii_uppercase())?,
            _ => {}
        }
    }
    have_freq.then_some(rule)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)
    };
    next.and_then(|n| n.pred_opt()).map(|d| d.day()).unwrap_or(28)
}

fn add_months(year: i32, month: u32, add: i64) -> (i32, u32) {
    let index = year as i64 * 12 + (month as i64 - 1) + add;
    (index.div_euclid(12) as i32, index.rem_euclid(12) as u32 + 1)
}

/// Dates in one month for BYMONTHDAY / BYDAY (with ordinals). `fallback`
/// is the day used when the rule names neither.
fn month_dates(rule: &Rule, year: i32, month: u32, fallback: u32) -> Vec<NaiveDate> {
    let last = days_in_month(year, month) as i32;
    let mut by_monthday: Vec<NaiveDate> = Vec::new();
    for d in &rule.by_monthday {
        let day = if *d < 0 { last + 1 + d } else { *d };
        if day >= 1 && day <= last {
            if let Some(date) = NaiveDate::from_ymd_opt(year, month, day as u32) {
                by_monthday.push(date);
            }
        }
    }
    let mut by_day: Vec<NaiveDate> = Vec::new();
    for (ord, wd) in &rule.by_day {
        let matching: Vec<NaiveDate> = (1..=last as u32)
            .filter_map(|d| NaiveDate::from_ymd_opt(year, month, d))
            .filter(|d| d.weekday() == *wd)
            .collect();
        match ord {
            None => by_day.extend(matching),
            Some(n) if *n > 0 => by_day.extend(matching.get(*n as usize - 1).copied()),
            Some(n) => {
                let from_end = (-n) as usize;
                if from_end >= 1 && from_end <= matching.len() {
                    by_day.push(matching[matching.len() - from_end]);
                }
            }
        }
    }
    let mut dates = match (rule.by_monthday.is_empty(), rule.by_day.is_empty()) {
        (false, false) => by_monthday.into_iter().filter(|d| by_day.contains(d)).collect(),
        (false, true) => by_monthday,
        (true, false) => by_day,
        (true, true) => NaiveDate::from_ymd_opt(year, month, fallback)
            .into_iter()
            .collect(),
    };
    dates.sort();
    dates.dedup();
    dates
}

/// Candidate dates of period `p` (not yet cut off at the start).
fn period_dates(rule: &Rule, start: NaiveDate, p: i64) -> Vec<NaiveDate> {
    let step = p * rule.interval;
    let mut dates = match rule.freq {
        Freq::Daily => {
            let d = start + Duration::days(step);
            let wd_ok = rule.by_day.is_empty() || rule.by_day.iter().any(|(_, w)| *w == d.weekday());
            let md_ok =
                rule.by_monthday.is_empty() || month_dates(rule, d.year(), d.month(), d.day()).contains(&d);
            if wd_ok && md_ok {
                vec![d]
            } else {
                vec![]
            }
        }
        Freq::Weekly => {
            let back = (start.weekday().num_days_from_monday() + 7 - rule.wkst.num_days_from_monday()) % 7;
            let week_start = start - Duration::days(back as i64) + Duration::weeks(step);
            let wds: Vec<Weekday> = if rule.by_day.is_empty() {
                vec![start.weekday()]
            } else {
                rule.by_day.iter().map(|(_, w)| *w).collect()
            };
            wds.into_iter()
                .map(|w| {
                    week_start
                        + Duration::days(
                            ((w.num_days_from_monday() + 7 - rule.wkst.num_days_from_monday()) % 7) as i64,
                        )
                })
                .collect()
        }
        Freq::Monthly => {
            let (y, m) = add_months(start.year(), start.month(), step);
            month_dates(rule, y, m, start.day())
        }
        Freq::Yearly => {
            let y = start.year() + step as i32;
            let months = if rule.by_month.is_empty() {
                vec![start.month()]
            } else {
                rule.by_month.clone()
            };
            months
                .into_iter()
                .flat_map(|m| month_dates(rule, y, m, start.day()))
                .collect()
        }
    };
    if !rule.by_month.is_empty() && rule.freq != Freq::Yearly {
        dates.retain(|d| rule.by_month.contains(&d.month()));
    }
    dates.sort();
    dates.dedup();
    dates
}

/// An instance of an event: start and end in the zone of the event.
pub struct Occurrence {
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
}

const MAX_PERIODS: i64 = 40_000;

/// All instances of `ev` that overlap `[from_ms, until_ms)`. `overridden`
/// are the starts (ms) of instances replaced by their own VEVENT.
pub fn occurrences(ev: &Ev, from_ms: i64, until_ms: i64, user: &str, overridden: &[i64]) -> Vec<Occurrence> {
    let len = ev.end - ev.start;
    let tz = ev.tz.as_deref();
    let excluded: Vec<i64> = ev
        .exdates
        .iter()
        .map(|s| to_ms(s.at, s.tz.as_deref().or(tz), user))
        .collect();
    let hit = |start: NaiveDateTime| -> Option<Occurrence> {
        let s = to_ms(start, tz, user);
        let e = to_ms(start + len, tz, user);
        if excluded.contains(&s) || overridden.contains(&s) {
            return None;
        }
        (e > from_ms && s < until_ms).then_some(Occurrence {
            start,
            end: start + len,
        })
    };
    let rule = ev.rrule.as_deref().and_then(parse_rule);
    let Some(rule) = rule else {
        return hit(ev.start).into_iter().collect();
    };
    // Naive cut-off: two days past the range covers every zone offset.
    let horizon = chrono::DateTime::from_timestamp_millis(until_ms)
        .map(|d| d.naive_utc() + Duration::days(2))
        .unwrap_or(NaiveDateTime::MAX);
    let mut out = Vec::new();
    let mut produced = 0u32;
    let time = ev.start.time();
    'periods: for p in 0..MAX_PERIODS {
        for date in period_dates(&rule, ev.start.date(), p) {
            let at = date.and_time(time);
            if at < ev.start {
                continue;
            }
            if at > horizon {
                // Candidates are sorted within a period and periods only
                // move forward, so nothing later can match.
                break 'periods;
            }
            if let Some(until) = &rule.until {
                let past = match until {
                    Until::Local(u) => at > *u,
                    Until::Utc(u) => to_ms(at, tz, user) > *u,
                };
                if past {
                    break 'periods;
                }
            }
            if let Some(max) = rule.count {
                if produced >= max {
                    break 'periods;
                }
            }
            produced += 1;
            out.extend(hit(at));
        }
    }
    out
}

/// Starts (ms) of the instances of `uid` that have their own VEVENT.
pub fn overridden_starts(events: &[Ev], uid: &str, user: &str) -> Vec<i64> {
    events
        .iter()
        .filter(|e| e.uid == uid)
        .filter_map(|e| e.rid.as_ref())
        .map(|r| to_ms(r.at, r.tz.as_deref(), user))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
    }

    fn cal(body: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{body}END:VCALENDAR\r\n")
    }

    fn day_ms(y: i32, m: u32, d: u32) -> i64 {
        to_ms(dt(y, m, d, 0, 0), None, "UTC")
    }

    #[test]
    fn reads_a_timed_event_with_folding_and_escapes() {
        let src = cal("BEGIN:VEVENT\r\nUID:a1\r\nSUMMARY:Planning\\, Q4 \\n\r\n  review\r\nLOCATION:Room 1\r\nDTSTART;TZID=Europe/Vienna:20261105T090000\r\nDTEND;TZID=Europe/Vienna:20261105T100000\r\nEND:VEVENT\r\n");
        let evs = parse(&src);
        assert_eq!(evs.len(), 1);
        let e = &evs[0];
        assert_eq!(e.title, "Planning, Q4 \n review");
        assert_eq!(e.location.as_deref(), Some("Room 1"));
        assert_eq!(e.tz.as_deref(), Some("Europe/Vienna"));
        assert!(!e.all_day);
        // 09:00 in Vienna (UTC+1 in November) is 08:00 UTC.
        assert_eq!(
            to_ms(e.start, e.tz.as_deref(), "UTC"),
            to_ms(dt(2026, 11, 5, 8, 0), Some("UTC"), "UTC")
        );
    }

    #[test]
    fn windows_zone_names_and_utc_values() {
        let src = cal("BEGIN:VEVENT\r\nUID:a\r\nDTSTART;TZID=W. Europe Standard Time:20260701T100000\r\nDTEND;TZID=W. Europe Standard Time:20260701T110000\r\nSUMMARY:x\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:b\r\nDTSTART:20260701T100000Z\r\nDURATION:PT90M\r\nSUMMARY:y\r\nEND:VEVENT\r\n");
        let evs = parse(&src);
        assert_eq!(evs[0].tz.as_deref(), Some("Europe/Berlin"));
        assert_eq!(evs[1].tz.as_deref(), Some("UTC"));
        assert_eq!(evs[1].end - evs[1].start, Duration::minutes(90));
    }

    #[test]
    fn all_day_events_and_alarms() {
        let src = cal("BEGIN:VEVENT\r\nUID:d\r\nSUMMARY:Holiday\r\nDTSTART;VALUE=DATE:20261224\r\nDTEND;VALUE=DATE:20261226\r\nBEGIN:VALARM\r\nTRIGGER:-PT15M\r\nSUMMARY:alarm text\r\nEND:VALARM\r\nEND:VEVENT\r\n");
        let e = &parse(&src)[0];
        assert!(e.all_day);
        assert_eq!(e.title, "Holiday");
        assert_eq!(e.end - e.start, Duration::days(2));
    }

    #[test]
    fn weekly_rule_with_exdate_and_override() {
        // Mondays and Wednesdays at 09:00 Vienna, five times; one skipped, one moved.
        let src = cal("BEGIN:VEVENT\r\nUID:w\r\nSUMMARY:Standup\r\nDTSTART;TZID=Europe/Vienna:20261102T090000\r\nDTEND;TZID=Europe/Vienna:20261102T091500\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,WE;COUNT=5\r\nEXDATE;TZID=Europe/Vienna:20261104T090000\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:w\r\nSUMMARY:Standup (moved)\r\nRECURRENCE-ID;TZID=Europe/Vienna:20261109T090000\r\nDTSTART;TZID=Europe/Vienna:20261109T140000\r\nDTEND;TZID=Europe/Vienna:20261109T141500\r\nEND:VEVENT\r\n");
        let evs = parse(&src);
        let master = &evs[0];
        let over = overridden_starts(&evs, "w", "UTC");
        let occ = occurrences(master, day_ms(2026, 11, 1), day_ms(2026, 12, 1), "UTC", &over);
        let days: Vec<(u32, u32)> = occ.iter().map(|o| (o.start.month(), o.start.day())).collect();
        // 2, (4 excluded), 9 (overridden), 11, 16 → count 5 covers 2,4,9,11,16.
        assert_eq!(days, vec![(11, 2), (11, 11), (11, 16)]);
        // The moved instance is its own event.
        assert_eq!(evs[1].rid.as_ref().unwrap().at, dt(2026, 11, 9, 9, 0));
    }

    #[test]
    fn dst_keeps_the_wall_clock_time() {
        // Daily 09:00 Vienna across the end of summer time (25 Oct 2026).
        let src = cal("BEGIN:VEVENT\r\nUID:x\r\nSUMMARY:x\r\nDTSTART;TZID=Europe/Vienna:20261023T090000\r\nDTEND;TZID=Europe/Vienna:20261023T100000\r\nRRULE:FREQ=DAILY;COUNT=4\r\nEND:VEVENT\r\n");
        let e = &parse(&src)[0];
        let occ = occurrences(e, day_ms(2026, 10, 1), day_ms(2026, 11, 30), "UTC", &[]);
        let utc_hours: Vec<i64> = occ
            .iter()
            .map(|o| (to_ms(o.start, e.tz.as_deref(), "UTC") / 3_600_000) % 24)
            .collect();
        assert_eq!(utc_hours, vec![7, 7, 8, 8]);
    }

    #[test]
    fn monthly_rules() {
        let mk = |rule: &str| {
            let src = cal(&format!("BEGIN:VEVENT\r\nUID:m\r\nSUMMARY:m\r\nDTSTART:20260115T100000Z\r\nDTEND:20260115T110000Z\r\nRRULE:{rule}\r\nEND:VEVENT\r\n"));
            let e = parse(&src).remove(0);
            occurrences(&e, day_ms(2026, 1, 1), day_ms(2026, 6, 1), "UTC", &[])
                .iter()
                .map(|o| (o.start.month(), o.start.day()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            mk("FREQ=MONTHLY"),
            vec![(1, 15), (2, 15), (3, 15), (4, 15), (5, 15)]
        );
        // Second Tuesday (13 January is before the start on the 15th); last Friday.
        assert_eq!(
            mk("FREQ=MONTHLY;BYDAY=2TU;COUNT=3"),
            vec![(2, 10), (3, 10), (4, 14)]
        );
        assert_eq!(
            mk("FREQ=MONTHLY;BYDAY=-1FR;COUNT=3"),
            vec![(1, 30), (2, 27), (3, 27)]
        );
        // The 31st exists only in some months.
        assert_eq!(mk("FREQ=MONTHLY;BYMONTHDAY=31"), vec![(1, 31), (3, 31), (5, 31)]);
        // Every other month, until a date.
        assert_eq!(
            mk("FREQ=MONTHLY;INTERVAL=2;UNTIL=20260331T000000Z"),
            vec![(1, 15), (3, 15)]
        );
    }

    #[test]
    fn yearly_and_interval_rules() {
        let src = cal("BEGIN:VEVENT\r\nUID:y\r\nSUMMARY:Birthday\r\nDTSTART;VALUE=DATE:20200229\r\nRRULE:FREQ=YEARLY\r\nEND:VEVENT\r\n");
        let e = parse(&src).remove(0);
        let occ = occurrences(&e, day_ms(2023, 1, 1), day_ms(2029, 1, 1), "UTC", &[]);
        // Only leap years have a 29 February.
        assert_eq!(
            occ.iter().map(|o| o.start.year()).collect::<Vec<_>>(),
            vec![2024, 2028]
        );

        let src = cal("BEGIN:VEVENT\r\nUID:i\r\nSUMMARY:i\r\nDTSTART:20261102T080000Z\r\nDTEND:20261102T090000Z\r\nRRULE:FREQ=DAILY;INTERVAL=3\r\nEND:VEVENT\r\n");
        let e = parse(&src).remove(0);
        let occ = occurrences(&e, day_ms(2026, 11, 1), day_ms(2026, 11, 12), "UTC", &[]);
        assert_eq!(
            occ.iter().map(|o| o.start.day()).collect::<Vec<_>>(),
            vec![2, 5, 8, 11]
        );
    }

    #[test]
    fn only_the_requested_range_and_cancelled_flag() {
        let src = cal("BEGIN:VEVENT\r\nUID:c\r\nSUMMARY:c\r\nSTATUS:CANCELLED\r\nTRANSP:TRANSPARENT\r\nDTSTART:20261110T080000Z\r\nDTEND:20261110T090000Z\r\nEND:VEVENT\r\n");
        let e = parse(&src).remove(0);
        assert!(e.cancelled && e.free);
        assert!(occurrences(&e, day_ms(2026, 11, 11), day_ms(2026, 11, 12), "UTC", &[]).is_empty());
        assert_eq!(
            occurrences(&e, day_ms(2026, 11, 10), day_ms(2026, 11, 11), "UTC", &[]).len(),
            1
        );
    }

    #[test]
    fn broken_input_does_not_panic() {
        assert!(parse("").is_empty());
        assert!(parse("garbage\r\nBEGIN:VEVENT\r\nDTSTART:nonsense\r\nEND:VEVENT\r\n").is_empty());
        assert!(parse_rule("FREQ=HOURLY").is_none());
        assert!(parse_rule("INTERVAL=2").is_none());
    }
}
