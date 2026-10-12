//! Report engine: filters entries and groups their time by project, tag,
//! cost center, billable flag or calendar period. Pure functions.
//!
//! Durations are sums of entry time clipped to the range: overlapping
//! entries each count fully, so reports add up per row. Grouping by tag
//! counts an entry once per tag, so tag rows can add up to more than the
//! total.

use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::locale::WeekStart;
use crate::model::TimeEntry;
use crate::summary::Range;
use crate::worktime::{day_bounds, local_date};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum ReportGroup {
    Project,
    Tag,
    CostCenter,
    Billable,
    Day,
    /// Key: first day of the week (`2026-10-05`); the week starts on the
    /// configured day.
    Week,
    /// Key `2026-10`.
    Month,
}

impl ReportGroup {
    fn is_period(self) -> bool {
        matches!(self, Self::Day | Self::Week | Self::Month)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase", default)]
pub struct ReportFilter {
    /// Empty = all. `null` matches entries without a project.
    pub project_ids: Vec<Option<String>>,
    /// Empty = all; otherwise entries with at least one of these tags.
    pub tag_ids: Vec<String>,
    /// Empty = all. `null` matches entries without a cost center.
    pub cost_center_ids: Vec<Option<String>>,
    pub billable: Option<bool>,
}

impl ReportFilter {
    pub fn matches(&self, e: &TimeEntry) -> bool {
        (self.project_ids.is_empty() || self.project_ids.contains(&e.project_id))
            && (self.tag_ids.is_empty() || e.tag_ids.iter().any(|t| self.tag_ids.contains(t)))
            && (self.cost_center_ids.is_empty() || self.cost_center_ids.contains(&e.cost_center_id))
            && self.billable.is_none_or(|b| b == e.billable)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ReportQuery {
    pub range: Range,
    #[serde(default)]
    pub filter: ReportFilter,
    /// Nesting levels, outermost first. Empty = totals only.
    #[serde(default)]
    pub group_by: Vec<ReportGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ReportRow {
    /// Id (project, tag, cost center), `"true"`/`"false"` (billable), or a
    /// period key (`2026-10-05`, `2026-W41`, `2026-10`). `None` = without.
    pub key: Option<String>,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub ms: i64,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub billable_ms: i64,
    pub entry_count: u32,
    pub children: Vec<ReportRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Report {
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub total_ms: i64,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub billable_ms: i64,
    pub entry_count: u32,
    pub rows: Vec<ReportRow>,
    /// Matching entries overlapping the range, by start time (not clipped).
    pub entries: Vec<TimeEntry>,
}

/// Part of an entry inside the range (and inside one local day when a
/// period grouping is used).
struct Piece<'a> {
    entry: &'a TimeEntry,
    ms: i64,
    date: NaiveDate,
}

pub fn build_report(
    entries: &[TimeEntry],
    query: &ReportQuery,
    now: i64,
    tz: Tz,
    week_start: WeekStart,
) -> Report {
    let split_days = query.group_by.iter().any(|g| g.is_period());
    let mut matching = Vec::new();
    let mut pieces = Vec::new();
    for entry in entries.iter().filter(|e| query.filter.matches(e)) {
        let start = entry.start_at.max(query.range.from);
        let end = entry.end_at.unwrap_or(now).min(query.range.to);
        if end <= start {
            continue;
        }
        matching.push(entry.clone());
        if split_days {
            let mut s = start;
            while s < end {
                let date = local_date(s, tz);
                let e = end.min(day_bounds(date, tz).1);
                // A DST gap can make a local day end where it starts.
                let e = if e <= s { end } else { e };
                pieces.push(Piece {
                    entry,
                    ms: e - s,
                    date,
                });
                s = e;
            }
        } else {
            pieces.push(Piece {
                entry,
                ms: end - start,
                date: local_date(start, tz),
            });
        }
    }
    let refs: Vec<&Piece> = pieces.iter().collect();
    let totals = totals(&refs);
    Report {
        total_ms: totals.0,
        billable_ms: totals.1,
        entry_count: totals.2,
        rows: group(&refs, &query.group_by, week_start),
        entries: matching,
    }
}

fn totals(pieces: &[&Piece]) -> (i64, i64, u32) {
    let ms = pieces.iter().map(|p| p.ms).sum();
    let billable = pieces.iter().filter(|p| p.entry.billable).map(|p| p.ms).sum();
    let count = pieces.iter().map(|p| &p.entry.id).collect::<HashSet<_>>().len();
    (ms, billable, count as u32)
}

fn keys(piece: &Piece, by: ReportGroup, week_start: WeekStart) -> Vec<Option<String>> {
    let e = piece.entry;
    match by {
        ReportGroup::Project => vec![e.project_id.clone()],
        ReportGroup::CostCenter => vec![e.cost_center_id.clone()],
        ReportGroup::Billable => vec![Some(e.billable.to_string())],
        ReportGroup::Tag if e.tag_ids.is_empty() => vec![None],
        ReportGroup::Tag => e.tag_ids.iter().cloned().map(Some).collect(),
        ReportGroup::Day => vec![Some(piece.date.format("%Y-%m-%d").to_string())],
        ReportGroup::Week => vec![Some(
            week_start.week_of(piece.date).format("%Y-%m-%d").to_string(),
        )],
        ReportGroup::Month => vec![Some(piece.date.format("%Y-%m").to_string())],
    }
}

fn group(pieces: &[&Piece], levels: &[ReportGroup], week_start: WeekStart) -> Vec<ReportRow> {
    let Some((&by, rest)) = levels.split_first() else {
        return vec![];
    };
    let mut buckets: HashMap<Option<String>, Vec<&Piece>> = HashMap::new();
    for &piece in pieces {
        for key in keys(piece, by, week_start) {
            buckets.entry(key).or_default().push(piece);
        }
    }
    let mut rows: Vec<ReportRow> = buckets
        .into_iter()
        .map(|(key, pieces)| {
            let (ms, billable_ms, entry_count) = totals(&pieces);
            ReportRow {
                key,
                ms,
                billable_ms,
                entry_count,
                children: group(&pieces, rest, week_start),
            }
        })
        .collect();
    if by.is_period() {
        rows.sort_by(|a, b| a.key.cmp(&b.key));
    } else {
        // Largest first; "without" last among equals.
        rows.sort_by(|a, b| {
            b.ms.cmp(&a.ms)
                .then_with(|| a.key.is_none().cmp(&b.key.is_none()))
                .then_with(|| a.key.cmp(&b.key))
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntrySource;

    const H: i64 = 3_600_000;
    /// 2026-10-05 00:00 Europe/Vienna (CEST, UTC+2).
    const MON: i64 = 1_791_151_200_000;

    fn entry(
        id: &str,
        project: Option<&str>,
        start: i64,
        end: Option<i64>,
        tags: &[&str],
        billable: bool,
    ) -> TimeEntry {
        TimeEntry {
            id: id.into(),
            workspace_id: "w".into(),
            user_id: "u".into(),
            project_id: project.map(Into::into),
            start_at: start,
            end_at: end,
            tz: "Europe/Vienna".into(),
            note: String::new(),
            billable,
            source: EntrySource::Manual,
            source_ref: None,
            locked: false,
            tag_ids: tags.iter().map(|t| t.to_string()).collect(),
            cost_center_id: project.map(|p| format!("cc-{p}")),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn query(group_by: Vec<ReportGroup>) -> ReportQuery {
        ReportQuery {
            range: Range {
                from: MON,
                to: MON + 7 * 24 * H,
            },
            filter: ReportFilter::default(),
            group_by,
        }
    }

    fn sample() -> Vec<TimeEntry> {
        vec![
            entry("1", Some("a"), MON + 8 * H, Some(MON + 10 * H), &["dev"], true),
            entry(
                "2",
                Some("b"),
                MON + 10 * H,
                Some(MON + 11 * H),
                &["dev", "meet"],
                false,
            ),
            // Crosses midnight into Tuesday.
            entry("3", Some("a"), MON + 23 * H, Some(MON + 25 * H), &[], true),
            // Starts before the range.
            entry("4", None, MON - H, Some(MON + H), &[], false),
        ]
    }

    #[test]
    fn groups_by_project_with_totals() {
        let r = build_report(
            &sample(),
            &query(vec![ReportGroup::Project]),
            0,
            chrono_tz::Europe::Vienna,
            WeekStart::Monday,
        );
        assert_eq!(r.total_ms, 6 * H);
        assert_eq!(r.billable_ms, 4 * H);
        assert_eq!(r.entry_count, 4);
        let rows: Vec<_> = r
            .rows
            .iter()
            .map(|row| (row.key.as_deref(), row.ms, row.entry_count))
            .collect();
        assert_eq!(rows, vec![(Some("a"), 4 * H, 2), (Some("b"), H, 1), (None, H, 1)]);
    }

    #[test]
    fn splits_days_in_local_time_and_nests_groups() {
        let r = build_report(
            &sample(),
            &query(vec![ReportGroup::Day, ReportGroup::Project]),
            0,
            chrono_tz::Europe::Vienna,
            WeekStart::Monday,
        );
        let days: Vec<_> = r
            .rows
            .iter()
            .map(|row| (row.key.clone().unwrap(), row.ms))
            .collect();
        assert_eq!(days, vec![("2026-10-05".into(), 5 * H), ("2026-10-06".into(), H)]);
        let tuesday = &r.rows[1];
        assert_eq!(tuesday.children.len(), 1);
        assert_eq!(tuesday.children[0].key.as_deref(), Some("a"));
        assert_eq!(tuesday.entry_count, 1);
    }

    #[test]
    fn week_and_month_keys() {
        let r = build_report(
            &sample(),
            &query(vec![ReportGroup::Week]),
            0,
            chrono_tz::Europe::Vienna,
            WeekStart::Monday,
        );
        assert_eq!(r.rows[0].key.as_deref(), Some("2026-10-05"));
        let r = build_report(
            &sample(),
            &query(vec![ReportGroup::Month]),
            0,
            chrono_tz::Europe::Vienna,
            WeekStart::Monday,
        );
        assert_eq!(r.rows[0].key.as_deref(), Some("2026-10"));
    }

    #[test]
    fn tags_count_an_entry_once_per_tag() {
        let r = build_report(
            &sample(),
            &query(vec![ReportGroup::Tag]),
            0,
            chrono_tz::Europe::Vienna,
            WeekStart::Monday,
        );
        let rows: Vec<_> = r.rows.iter().map(|row| (row.key.as_deref(), row.ms)).collect();
        assert_eq!(rows, vec![(Some("dev"), 3 * H), (None, 3 * H), (Some("meet"), H)]);
    }

    #[test]
    fn filters_combine() {
        let mut q = query(vec![ReportGroup::CostCenter]);
        q.filter.billable = Some(true);
        q.filter.tag_ids = vec!["dev".into()];
        let r = build_report(&sample(), &q, 0, chrono_tz::Europe::Vienna, WeekStart::Monday);
        assert_eq!(r.total_ms, 2 * H);
        assert_eq!(r.entries.len(), 1);
        assert_eq!(r.rows[0].key.as_deref(), Some("cc-a"));

        let mut q = query(vec![]);
        q.filter.project_ids = vec![None];
        let r = build_report(&sample(), &q, 0, chrono_tz::Europe::Vienna, WeekStart::Monday);
        assert_eq!(r.total_ms, H);
        assert!(r.rows.is_empty());
    }

    #[test]
    fn running_entries_count_until_now() {
        let entries = vec![entry("r", Some("a"), MON + H, None, &[], false)];
        let r = build_report(
            &entries,
            &query(vec![]),
            MON + 3 * H,
            chrono_tz::Europe::Vienna,
            WeekStart::Monday,
        );
        assert_eq!(r.total_ms, 2 * H);
    }
}
