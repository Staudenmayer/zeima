//! Reads time entries exported from other tools (Toggl, Clockify, or any CSV
//! with the same columns). Pure: no database. `Core::import_entries` writes
//! what this returns.
//!
//! The columns are found by their header names (case does not matter):
//! `Project`, `Client`, `Description`, `Billable`, `Start date`, `Start
//! time`, `End date`, `End time`, `Tags`. Toggl's and Clockify's detailed
//! exports both have them. Dates are `2026-10-05`, `05.10.2026` or
//! `10/05/2026`: which order is meant is the caller's choice, because
//! `05/10/2026` cannot be told apart.

use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum DateOrder {
    /// `2026-10-05`
    Ymd,
    /// `05.10.2026`, `05/10/2026`
    Dmy,
    /// `10/05/2026`
    Mdy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedEntry {
    pub project: Option<String>,
    pub client: Option<String>,
    pub note: String,
    pub billable: bool,
    pub tags: Vec<String>,
    pub start_at: i64,
    pub end_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SkippedRow {
    /// Line in the file, counting the header as 1.
    pub line: u32,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct ParsedImport {
    pub entries: Vec<ParsedEntry>,
    pub skipped: Vec<SkippedRow>,
}

/// Splits CSV text into rows of fields (RFC 4180: quotes, doubled quotes, line breaks inside quotes).
fn rows(text: &str, delimiter: char) -> Vec<(u32, Vec<String>)> {
    let mut out = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut line = 1u32;
    let mut row_line = 1u32;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (d, false) if d == delimiter => row.push(std::mem::take(&mut field)),
            ('\r', false) => {}
            ('\n', false) => {
                row.push(std::mem::take(&mut field));
                if row.iter().any(|f| !f.trim().is_empty()) {
                    out.push((row_line, std::mem::take(&mut row)));
                } else {
                    row.clear();
                }
                line += 1;
                row_line = line;
            }
            (other, _) => {
                if other == '\n' {
                    line += 1;
                }
                field.push(other);
            }
        }
    }
    row.push(field);
    if row.iter().any(|f| !f.trim().is_empty()) {
        out.push((row_line, row));
    }
    out
}

fn parse_date(s: &str, order: DateOrder) -> Option<NaiveDate> {
    let parts: Vec<u32> = s
        .trim()
        .split(['-', '/', '.'])
        .map(|p| p.trim().parse().ok())
        .collect::<Option<_>>()?;
    let [a, b, c] = parts[..] else { return None };
    let (y, m, d) = match order {
        DateOrder::Ymd => (a, b, c),
        DateOrder::Dmy => (c, b, a),
        DateOrder::Mdy => (c, a, b),
    };
    // Two-digit years are 20xx.
    let y = if y < 100 { 2000 + y } else { y };
    NaiveDate::from_ymd_opt(y as i32, m, d)
}

fn parse_time(s: &str) -> Option<NaiveTime> {
    let s = s.trim().to_ascii_uppercase();
    let (body, pm) = match s.strip_suffix("PM") {
        Some(b) => (b.trim(), Some(true)),
        None => match s.strip_suffix("AM") {
            Some(b) => (b.trim(), Some(false)),
            None => (s.as_str(), None),
        },
    };
    let mut it = body.split(':').map(|p| p.trim().parse::<u32>().ok());
    let mut h = it.next()??;
    let m = it.next().flatten().unwrap_or(0);
    let sec = it.next().flatten().unwrap_or(0);
    if let Some(pm) = pm {
        if !(1..=12).contains(&h) {
            return None;
        }
        h = h % 12 + if pm { 12 } else { 0 };
    }
    NaiveTime::from_hms_opt(h, m, sec)
}

fn local_ms(date: NaiveDate, time: NaiveTime, tz: chrono_tz::Tz) -> Option<i64> {
    let naive: NaiveDateTime = date.and_time(time);
    tz.from_local_datetime(&naive)
        .earliest()
        .map(|d| d.timestamp_millis())
}

fn yes(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "yes" | "true" | "1" | "ja" | "y"
    )
}

/// Reads an export. `tz` is the zone the times in the file are in.
pub fn parse_import(text: &str, order: DateOrder, tz: &str) -> Result<ParsedImport> {
    let zone: chrono_tz::Tz = tz
        .parse()
        .map_err(|_| Error::validation("tz", format!("unknown time zone '{tz}'")))?;
    let first_line = text.lines().next().unwrap_or("");
    let delimiter = if first_line.matches(';').count() > first_line.matches(',').count() {
        ';'
    } else {
        ','
    };
    let mut all = rows(text, delimiter).into_iter();
    let Some((_, header)) = all.next() else {
        return Err(Error::validation("file", "the file is empty"));
    };
    let find = |names: &[&str]| {
        header
            .iter()
            .position(|h| names.iter().any(|n| h.trim().eq_ignore_ascii_case(n)))
    };
    let (Some(sd), Some(st), Some(ed), Some(et)) = (
        find(&["start date"]),
        find(&["start time"]),
        find(&["end date"]),
        find(&["end time"]),
    ) else {
        return Err(Error::validation(
            "file",
            "this does not look like a Toggl or Clockify export: the columns Start date, Start time, End date and End time are missing",
        ));
    };
    let (project, client, note, billable, tags) = (
        find(&["project", "projekt"]),
        find(&["client", "kunde"]),
        find(&["description", "beschreibung"]),
        find(&["billable", "abrechenbar"]),
        find(&["tags", "tag"]),
    );
    let cell = |row: &[String], i: Option<usize>| {
        i.and_then(|i| row.get(i))
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };

    let mut out = ParsedImport::default();
    for (line, row) in all {
        let skip = |out: &mut ParsedImport, reason: &str| {
            out.skipped.push(SkippedRow {
                line,
                reason: reason.into(),
            })
        };
        let when = |d: usize, t: usize| {
            let date = parse_date(row.get(d)?, order)?;
            let time = parse_time(row.get(t)?)?;
            local_ms(date, time, zone)
        };
        let (Some(start_at), Some(end_at)) = (when(sd, st), when(ed, et)) else {
            skip(
                &mut out,
                "the date or time could not be read (check the date order)",
            );
            continue;
        };
        if end_at <= start_at {
            skip(&mut out, "the entry ends before it starts");
            continue;
        }
        let name = cell(&row, project);
        let client_name = cell(&row, client);
        out.entries.push(ParsedEntry {
            project: (!name.is_empty()).then_some(name),
            client: (!client_name.is_empty()).then_some(client_name),
            note: cell(&row, note),
            billable: yes(&cell(&row, billable)),
            tags: cell(&row, tags)
                .split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect(),
            start_at,
            end_at,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 3_600_000;

    #[test]
    fn reads_a_toggl_export() {
        let csv = "User,Email,Client,Project,Task,Description,Billable,Start date,Start time,End date,End time,Duration,Tags,Amount (EUR)\n\
Ada,ada@example.com,Acme,Website,,\"Fix, then \"\"ship\"\"\",Yes,2026-10-05,09:00:00,2026-10-05,11:30:00,02:30:00,\"dev, urgent\",\n\
Ada,ada@example.com,,,,Planning,No,2026-10-05,13:00:00,2026-10-05,14:00:00,01:00:00,,\n";
        let parsed = parse_import(csv, DateOrder::Ymd, "Europe/Vienna").unwrap();
        assert!(parsed.skipped.is_empty());
        let e = &parsed.entries[0];
        assert_eq!(
            (
                e.project.as_deref(),
                e.client.as_deref(),
                e.note.as_str(),
                e.billable
            ),
            (Some("Website"), Some("Acme"), "Fix, then \"ship\"", true)
        );
        assert_eq!(e.tags, vec!["dev", "urgent"]);
        // 09:00 Vienna in October is 07:00 UTC.
        assert_eq!(e.end_at - e.start_at, 2 * HOUR + HOUR / 2);
        assert_eq!(
            e.start_at,
            chrono::Utc
                .with_ymd_and_hms(2026, 10, 5, 7, 0, 0)
                .unwrap()
                .timestamp_millis()
        );
        assert_eq!(parsed.entries[1].project, None);
        assert!(!parsed.entries[1].billable);
    }

    #[test]
    fn reads_clockify_with_us_dates_and_12_hour_times() {
        let csv = "\"Project\",\"Client\",\"Description\",\"Task\",\"User\",\"Billable\",\"Start Date\",\"Start Time\",\"End Date\",\"End Time\",\"Tags\"\n\
\"Site\",\"\",\"Review\",\"\",\"Ada\",\"Yes\",\"10/05/2026\",\"09:30:00 AM\",\"10/05/2026\",\"12:15:00 PM\",\"qa\"\n\
\"Site\",\"\",\"Night\",\"\",\"Ada\",\"No\",\"10/05/2026\",\"11:00:00 PM\",\"10/06/2026\",\"12:30:00 AM\",\"\"\n";
        let parsed = parse_import(csv, DateOrder::Mdy, "UTC").unwrap();
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(
            parsed.entries[0].end_at - parsed.entries[0].start_at,
            2 * HOUR + 45 * 60_000
        );
        assert_eq!(
            parsed.entries[1].end_at - parsed.entries[1].start_at,
            HOUR + 30 * 60_000,
            "across midnight, 12:30 AM is half past midnight"
        );
    }

    #[test]
    fn german_exports_use_semicolons_and_day_first_dates() {
        let csv = "Projekt;Beschreibung;Abrechenbar;Start date;Start time;End date;End time\nWeb;Test;Ja;05.10.2026;08:00;05.10.2026;09:00\n";
        let parsed = parse_import(csv, DateOrder::Dmy, "UTC").unwrap();
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(
            (
                parsed.entries[0].project.as_deref(),
                parsed.entries[0].billable,
                parsed.entries[0].note.as_str()
            ),
            (Some("Web"), true, "Test")
        );
    }

    #[test]
    fn bad_rows_are_skipped_with_their_line_number_and_missing_columns_refused() {
        let csv = "Project,Description,Start date,Start time,End date,End time\nA,ok,2026-10-05,09:00,2026-10-05,10:00\nA,backwards,2026-10-05,10:00,2026-10-05,09:00\nA,\"garbled,2026-13-45,x,y,z\n";
        let parsed = parse_import(csv, DateOrder::Ymd, "UTC").unwrap();
        assert_eq!(parsed.entries.len(), 1);
        // The backwards row (line 3) and the garbled one (line 4, a quote that never closes).
        assert_eq!(
            parsed.skipped.iter().map(|s| s.line).collect::<Vec<_>>(),
            vec![3, 4]
        );
        assert!(parse_import("Foo,Bar\n1,2\n", DateOrder::Ymd, "UTC").is_err());
        assert!(parse_import("", DateOrder::Ymd, "UTC").is_err());
        assert!(parse_import("a\n", DateOrder::Ymd, "Mars/Base").is_err());
    }

    #[test]
    fn dates_depend_on_the_chosen_order() {
        let d = |s, o| parse_date(s, o);
        assert_eq!(
            d("05/10/2026", DateOrder::Dmy),
            NaiveDate::from_ymd_opt(2026, 10, 5)
        );
        assert_eq!(
            d("05/10/2026", DateOrder::Mdy),
            NaiveDate::from_ymd_opt(2026, 5, 10)
        );
        assert_eq!(
            d("2026-10-05", DateOrder::Ymd),
            NaiveDate::from_ymd_opt(2026, 10, 5)
        );
        assert_eq!(d("31/02/2026", DateOrder::Dmy), None);
    }
}
