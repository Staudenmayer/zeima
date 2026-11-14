//! Meetings and their repeats (SPEC §5.12). Pure: the repeat rule and the
//! times it gives. The server computes the same occurrences (free/busy);
//! both run `packages/shared-fixtures/meetings/occurrences.json`.
//!
//! Supported rules: `FREQ=DAILY|WEEKLY|MONTHLY`, `INTERVAL`, `COUNT`,
//! `UNTIL=YYYYMMDD` (inclusive) and, for weekly rules, `BYDAY=MO,WE`.
//! Monthly meetings repeat on the day of the month they started; months
//! without that day are skipped.

use chrono::{Datelike, Duration, LocalResult, NaiveDate, TimeZone, Weekday};
use chrono_tz::Tz;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Freq {
    Daily,
    Weekly,
    Monthly,
}

#[derive(Debug)]
struct Rule {
    freq: Freq,
    interval: i64,
    count: Option<u32>,
    until: Option<NaiveDate>,
    by_day: Vec<Weekday>,
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

fn parse(rule: &str) -> Option<Rule> {
    let mut r = Rule {
        freq: Freq::Daily,
        interval: 1,
        count: None,
        until: None,
        by_day: Vec::new(),
    };
    let mut have = false;
    for part in rule.split(';') {
        let (k, v) = part.split_once('=')?;
        match k.to_ascii_uppercase().as_str() {
            "FREQ" => {
                r.freq = match v.to_ascii_uppercase().as_str() {
                    "DAILY" => Freq::Daily,
                    "WEEKLY" => Freq::Weekly,
                    "MONTHLY" => Freq::Monthly,
                    _ => return None,
                };
                have = true;
            }
            "INTERVAL" => r.interval = v.parse::<i64>().ok()?.max(1),
            "COUNT" => r.count = Some(v.parse().ok()?),
            "UNTIL" => r.until = Some(NaiveDate::parse_from_str(v.get(..8)?, "%Y%m%d").ok()?),
            "BYDAY" => {
                r.by_day = v
                    .split(',')
                    .map(|d| weekday_of(&d.trim().to_ascii_uppercase()))
                    .collect::<Option<_>>()?
            }
            _ => {}
        }
    }
    have.then_some(r)
}

/// Whether `rule` is a repeat rule this module understands.
pub fn is_valid_rule(rule: &str) -> bool {
    parse(rule).is_some()
}

fn local_ms(date: NaiveDate, seconds_of_day: u32, tz: Tz) -> Option<i64> {
    let naive = date.and_hms_opt(
        seconds_of_day / 3600,
        seconds_of_day / 60 % 60,
        seconds_of_day % 60,
    )?;
    match tz.from_local_datetime(&naive) {
        LocalResult::Single(d) => Some(d.timestamp_millis()),
        LocalResult::Ambiguous(first, _) => Some(first.timestamp_millis()),
        LocalResult::None => tz
            .from_local_datetime(&(naive + Duration::hours(1)))
            .earliest()
            .map(|d| d.timestamp_millis()),
    }
}

/// Start and end (unix ms) of each time the meeting happens that overlaps
/// `[from, to)`. `start_at`/`end_at` are the first occurrence; the repeats keep
/// its local time of day (in `tz`) and its length.
pub fn occurrences(
    start_at: i64,
    end_at: i64,
    tz: &str,
    rrule: Option<&str>,
    from: i64,
    to: i64,
) -> Vec<(i64, i64)> {
    let len = (end_at - start_at).max(0);
    let overlaps = |s: i64| s < to && s + len > from;
    let Some(rule) = rrule.and_then(parse) else {
        return if overlaps(start_at) {
            vec![(start_at, start_at + len)]
        } else {
            vec![]
        };
    };
    let Ok(zone) = tz.parse::<Tz>() else {
        return if overlaps(start_at) {
            vec![(start_at, start_at + len)]
        } else {
            vec![]
        };
    };
    let Some(first) = chrono::DateTime::from_timestamp_millis(start_at).map(|d| d.with_timezone(&zone))
    else {
        return vec![];
    };
    let (d0, seconds) = (first.date_naive(), first.time().num_seconds_from_midnight_u32());
    let mut out = Vec::new();
    let mut produced = 0u32;
    // The dates of period `p`, in order.
    let dates = |p: i64| -> Vec<NaiveDate> {
        match rule.freq {
            Freq::Daily => vec![d0 + Duration::days(p * rule.interval)],
            Freq::Weekly => {
                let monday = d0 - Duration::days(i64::from(d0.weekday().num_days_from_monday()))
                    + Duration::weeks(p * rule.interval);
                let mut days: Vec<Weekday> = if rule.by_day.is_empty() {
                    vec![d0.weekday()]
                } else {
                    rule.by_day.clone()
                };
                days.sort_by_key(|d| d.num_days_from_monday());
                days.dedup();
                days.into_iter()
                    .map(|d| monday + Duration::days(i64::from(d.num_days_from_monday())))
                    .collect()
            }
            Freq::Monthly => {
                let index = i64::from(d0.year()) * 12 + i64::from(d0.month0()) + p * rule.interval;
                NaiveDate::from_ymd_opt(
                    index.div_euclid(12) as i32,
                    index.rem_euclid(12) as u32 + 1,
                    d0.day(),
                )
                .into_iter()
                .collect()
            }
        }
    };
    'periods: for p in 0..5000 {
        for date in dates(p) {
            if date < d0 {
                continue;
            }
            if rule.until.is_some_and(|u| date > u) || rule.count.is_some_and(|c| produced >= c) {
                break 'periods;
            }
            let Some(s) = local_ms(date, seconds, zone) else {
                continue;
            };
            if s >= to {
                break 'periods;
            }
            produced += 1;
            if overlaps(s) {
                out.push((s, s + len));
            }
        }
    }
    out
}

use chrono::Timelike as _;
trait SecondsOfDay {
    fn num_seconds_from_midnight_u32(&self) -> u32;
}
impl SecondsOfDay for chrono::NaiveTime {
    fn num_seconds_from_midnight_u32(&self) -> u32 {
        self.num_seconds_from_midnight()
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize)]
    struct Case {
        name: String,
        #[serde(rename = "startAt")]
        start_at: i64,
        #[serde(rename = "endAt")]
        end_at: i64,
        tz: String,
        rrule: Option<String>,
        from: i64,
        to: i64,
        expected: Vec<[i64; 2]>,
    }

    #[test]
    fn matches_the_shared_vectors() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../packages/shared-fixtures/meetings/occurrences.json"
        );
        let cases: Vec<Case> = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(cases.len() >= 8);
        for c in cases {
            let got = occurrences(c.start_at, c.end_at, &c.tz, c.rrule.as_deref(), c.from, c.to);
            assert_eq!(
                got,
                c.expected.iter().map(|e| (e[0], e[1])).collect::<Vec<_>>(),
                "{}",
                c.name
            );
        }
    }

    #[test]
    fn rules_are_checked() {
        assert!(is_valid_rule("FREQ=WEEKLY;BYDAY=MO,WE;COUNT=4"));
        assert!(!is_valid_rule("FREQ=HOURLY"));
        assert!(!is_valid_rule("INTERVAL=2"));
        assert!(!is_valid_rule("FREQ=WEEKLY;BYDAY=XX"));
    }
}
