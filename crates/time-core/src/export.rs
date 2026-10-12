//! Turns a report into tables for CSV/XLSX export. Pure functions; file
//! formats that need extra crates (XLSX) are written by the app.

use std::collections::HashMap;

use chrono::{DateTime, Datelike, NaiveDate};
use chrono_tz::Tz;

use crate::locale::Formats;
use crate::model::{Activity, CostCenter, Project, Tag};
use crate::report::{Report, ReportGroup, ReportRow};

#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Text(String),
    /// Decimal hours.
    Hours(f64),
    Int(i64),
    Date(NaiveDate),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub name: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
}

/// Names for ids found in reports.
pub struct Lookup<'a> {
    activities: HashMap<&'a str, &'a Activity>,
    projects: HashMap<&'a str, &'a Project>,
    tags: HashMap<&'a str, &'a Tag>,
    cost_centers: HashMap<&'a str, &'a CostCenter>,
}

impl<'a> Lookup<'a> {
    pub fn new(
        activities: &'a [Activity],
        projects: &'a [Project],
        tags: &'a [Tag],
        cost_centers: &'a [CostCenter],
    ) -> Self {
        Self {
            activities: activities.iter().map(|a| (a.id.as_str(), a)).collect(),
            projects: projects.iter().map(|p| (p.id.as_str(), p)).collect(),
            tags: tags.iter().map(|t| (t.id.as_str(), t)).collect(),
            cost_centers: cost_centers.iter().map(|c| (c.id.as_str(), c)).collect(),
        }
    }
}

/// Column and value texts (German or English) and regional formats.
struct Texts {
    de: bool,
    f: Formats,
}

impl Texts {
    fn t(&self, en: &'static str, de: &'static str) -> String {
        if self.de { de } else { en }.to_string()
    }

    fn group(&self, g: ReportGroup) -> String {
        match g {
            ReportGroup::Activity => self.t("Activity", "Tätigkeit"),
            ReportGroup::Project => self.t("Project", "Projekt"),
            ReportGroup::Tag => self.t("Tag", "Tag"),
            ReportGroup::CostCenter => self.t("Cost center", "Kostenstelle"),
            ReportGroup::Billable => self.t("Billable", "Verrechenbar"),
            ReportGroup::Day => self.t("Day", "Tag"),
            ReportGroup::Week => self.t("Week", "Woche"),
            ReportGroup::Month => self.t("Month", "Monat"),
        }
    }

    fn yes_no(&self, v: bool) -> String {
        match (v, self.de) {
            (true, true) => "Ja",
            (false, true) => "Nein",
            (true, false) => "Yes",
            (false, false) => "No",
        }
        .into()
    }
}

fn hours(ms: i64) -> f64 {
    // Rounded to minutes, shown as hours with two decimals.
    (ms as f64 / 60_000.0).round() / 60.0
}

fn cost_center_label(cc: &CostCenter) -> String {
    format!("{} {}", cc.code, cc.name)
}

fn key_label(key: &Option<String>, by: ReportGroup, lookup: &Lookup, texts: &Texts) -> String {
    let Some(key) = key else {
        return match by {
            ReportGroup::Activity => texts.t("No activity", "Keine Tätigkeit"),
            ReportGroup::Project => texts.t("No project", "Kein Projekt"),
            ReportGroup::Tag => texts.t("No tag", "Kein Tag"),
            ReportGroup::CostCenter => texts.t("No cost center", "Keine Kostenstelle"),
            _ => String::new(),
        };
    };
    match by {
        ReportGroup::Activity => lookup
            .activities
            .get(key.as_str())
            .map_or_else(|| key.clone(), |a| a.name.clone()),
        ReportGroup::Project => lookup
            .projects
            .get(key.as_str())
            .map_or_else(|| key.clone(), |p| p.name.clone()),
        ReportGroup::Tag => lookup
            .tags
            .get(key.as_str())
            .map_or_else(|| key.clone(), |t| t.name.clone()),
        ReportGroup::CostCenter => lookup
            .cost_centers
            .get(key.as_str())
            .map_or_else(|| key.clone(), |c| cost_center_label(c)),
        ReportGroup::Billable => texts.yes_no(key == "true"),
        ReportGroup::Day => key
            .parse::<NaiveDate>()
            .map_or_else(|_| key.clone(), |d| texts.f.date(d)),
        ReportGroup::Week => key
            .parse::<NaiveDate>()
            .map_or_else(|_| key.clone(), |d| texts.f.week(d)),
        ReportGroup::Month => format!("{key}-01")
            .parse::<NaiveDate>()
            .map_or_else(|_| key.clone(), |d| texts.f.month_year(d.year(), d.month())),
    }
}

/// Grouped totals: one column per grouping level, then hours. Every level
/// gets a row, so subtotals are visible.
pub fn summary_table(report: &Report, group_by: &[ReportGroup], lookup: &Lookup, f: &Formats) -> Table {
    let texts = Texts { de: f.german, f: *f };
    let mut headers: Vec<String> = group_by.iter().map(|&g| texts.group(g)).collect();
    headers.push(texts.t("Hours", "Stunden"));
    headers.push(texts.t("Billable hours", "Verrechenbare Stunden"));
    headers.push(texts.t("Entries", "Einträge"));

    fn walk(
        rows: &[ReportRow],
        level: usize,
        group_by: &[ReportGroup],
        lookup: &Lookup,
        texts: &Texts,
        out: &mut Vec<Vec<Cell>>,
    ) {
        for row in rows {
            let mut cells = vec![Cell::Text(String::new()); group_by.len()];
            cells[level] = Cell::Text(key_label(&row.key, group_by[level], lookup, texts));
            cells.push(Cell::Hours(hours(row.ms)));
            cells.push(Cell::Hours(hours(row.billable_ms)));
            cells.push(Cell::Int(row.entry_count.into()));
            out.push(cells);
            walk(&row.children, level + 1, group_by, lookup, texts, out);
        }
    }

    let mut rows = Vec::new();
    walk(&report.rows, 0, group_by, lookup, &texts, &mut rows);
    let mut total = vec![Cell::Text(String::new()); group_by.len()];
    if let Some(first) = total.first_mut() {
        *first = Cell::Text(texts.t("Total", "Summe"));
    }
    total.push(Cell::Hours(hours(report.total_ms)));
    total.push(Cell::Hours(hours(report.billable_ms)));
    total.push(Cell::Int(report.entry_count.into()));
    rows.push(total);
    Table {
        name: texts.t("Summary", "Übersicht"),
        headers,
        rows,
    }
}

/// One row per entry, times in local time. Running entries end at `now`.
pub fn entries_table(report: &Report, lookup: &Lookup, tz: Tz, now: i64, f: &Formats) -> Table {
    let texts = Texts { de: f.german, f: *f };
    let headers = vec![
        texts.t("Date", "Datum"),
        texts.t("Start", "Beginn"),
        texts.t("End", "Ende"),
        texts.t("Hours", "Stunden"),
        texts.t("Activity", "Tätigkeit"),
        texts.t("Project", "Projekt"),
        texts.t("Project code", "Projektnummer"),
        texts.t("Client", "Kunde"),
        texts.t("Tags", "Tags"),
        texts.t("Cost center", "Kostenstelle"),
        texts.t("Cost center name", "Kostenstellenname"),
        texts.t("Billable", "Verrechenbar"),
        texts.t("Note", "Notiz"),
    ];
    let local = |ms: i64| DateTime::from_timestamp_millis(ms).map(|d| d.with_timezone(&tz));
    let rows = report
        .entries
        .iter()
        .map(|e| {
            let start = local(e.start_at);
            let end_ms = e.end_at.unwrap_or(now);
            let end = local(end_ms);
            let activity = e.activity_id.as_deref().and_then(|id| lookup.activities.get(id));
            let project = e.project_id.as_deref().and_then(|id| lookup.projects.get(id));
            let cc = e
                .cost_center_id
                .as_deref()
                .and_then(|id| lookup.cost_centers.get(id));
            let tags: Vec<&str> = e
                .tag_ids
                .iter()
                .filter_map(|id| lookup.tags.get(id.as_str()).map(|t| t.name.as_str()))
                .collect();
            vec![
                start.map_or(Cell::Text(String::new()), |d| Cell::Date(d.date_naive())),
                Cell::Text(start.map(|d| f.time(d.time())).unwrap_or_default()),
                Cell::Text(if e.end_at.is_some() {
                    end.map(|d| f.time(d.time())).unwrap_or_default()
                } else {
                    String::new()
                }),
                Cell::Hours(hours(end_ms - e.start_at)),
                Cell::Text(activity.map(|a| a.name.clone()).unwrap_or_default()),
                Cell::Text(project.map(|p| p.name.clone()).unwrap_or_default()),
                Cell::Text(project.and_then(|p| p.alias.clone()).unwrap_or_default()),
                Cell::Text(project.and_then(|p| p.client.clone()).unwrap_or_default()),
                Cell::Text(tags.join(", ")),
                Cell::Text(cc.map(|c| c.code.clone()).unwrap_or_default()),
                Cell::Text(cc.map(|c| c.name.clone()).unwrap_or_default()),
                Cell::Text(texts.yes_no(e.billable)),
                Cell::Text(e.note.clone()),
            ]
        })
        .collect();
    Table {
        name: texts.t("Entries", "Einträge"),
        headers,
        rows,
    }
}

/// CSV as spreadsheet apps expect it for the number format: with a decimal
/// comma the separator is `;`, otherwise `,`. Dates use the date format.
/// Starts with a UTF-8 BOM so Excel detects the encoding.
pub fn to_csv(table: &Table, f: &Formats) -> String {
    let sep = if f.decimal_comma() { ';' } else { ',' };
    let quote = |s: &str| {
        if s.contains([sep, '"', '\n', '\r']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };
    let cell = |c: &Cell| match c {
        Cell::Text(s) => quote(s),
        Cell::Hours(h) => f.decimal(*h, 2),
        Cell::Int(i) => i.to_string(),
        Cell::Date(d) => quote(&f.date(*d)),
    };
    let mut out = String::from('\u{feff}');
    let line = |cells: Vec<String>| cells.join(&sep.to_string()) + "\r\n";
    out.push_str(&line(table.headers.iter().map(|h| quote(h)).collect()));
    for row in &table.rows {
        out.push_str(&line(row.iter().map(cell).collect()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locale::{system_formats, DateFormat, NumberFormat, TimeFormat, WeekStart};

    #[test]
    fn csv_quotes_and_localizes_numbers() {
        let table = Table {
            name: "t".into(),
            headers: vec!["Note".into(), "Hours".into()],
            rows: vec![vec![Cell::Text("a; \"b\"".into()), Cell::Hours(1.5)]],
        };
        let de = Formats::new(
            "de",
            system_formats("de-DE"),
            DateFormat::Auto,
            TimeFormat::Auto,
            NumberFormat::Auto,
            WeekStart::Monday,
        );
        let en = Formats::new(
            "en",
            system_formats("en-US"),
            DateFormat::Auto,
            TimeFormat::Auto,
            NumberFormat::Auto,
            WeekStart::Monday,
        );
        assert_eq!(
            to_csv(&table, &de),
            "\u{feff}Note;Hours\r\n\"a; \"\"b\"\"\";1,50\r\n"
        );
        assert_eq!(
            to_csv(&table, &en),
            "\u{feff}Note,Hours\r\n\"a; \"\"b\"\"\",1.50\r\n"
        );
        // German text with a decimal point: `,` separates.
        let mixed = Formats::new(
            "de",
            system_formats("de-DE"),
            DateFormat::Ymd,
            TimeFormat::Auto,
            NumberFormat::Point,
            WeekStart::Monday,
        );
        let dated = Table {
            name: "t".into(),
            headers: vec!["Date".into()],
            rows: vec![vec![Cell::Date("2026-10-05".parse().unwrap()), Cell::Hours(2.0)]],
        };
        assert_eq!(to_csv(&dated, &mixed), "\u{feff}Date\r\n2026-10-05,2.00\r\n");
        assert_eq!(to_csv(&dated, &de), "\u{feff}Date\r\n05.10.2026;2,00\r\n");
    }

    #[test]
    fn hours_are_rounded_to_minutes() {
        assert_eq!(hours(90 * 60_000 + 29_000), 1.5);
        assert!((hours(20 * 60_000) - 0.3333).abs() < 0.001);
    }
}
