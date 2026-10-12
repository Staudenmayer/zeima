//! Tracked-time summaries over a range. Pure functions.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::model::TimeEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Range {
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub from: i64,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub to: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ProjectTotal {
    pub project_id: Option<String>,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub range: Range,
    /// Wall-clock time covered by at least one entry. Overlaps count once.
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub total_ms: i64,
    /// Per project, sorted by time descending. Overlapping entries on
    /// different projects each count fully here.
    pub by_project: Vec<ProjectTotal>,
    pub entry_count: u32,
}

/// Clips `[start, end)` to the range. Running entries end at `now`.
fn clip(entry: &TimeEntry, range: Range, now: i64) -> Option<(i64, i64)> {
    let end = entry.end_at.unwrap_or(now);
    let s = entry.start_at.max(range.from);
    let e = end.min(range.to);
    (e > s).then_some((s, e))
}

/// Total length of the union of the intervals.
pub fn union_ms(mut intervals: Vec<(i64, i64)>) -> i64 {
    intervals.sort_unstable();
    let mut total = 0;
    let mut current: Option<(i64, i64)> = None;
    for (s, e) in intervals {
        match current {
            Some((cs, ce)) if s <= ce => current = Some((cs, ce.max(e))),
            Some((cs, ce)) => {
                total += ce - cs;
                current = Some((s, e));
            }
            None => current = Some((s, e)),
        }
    }
    if let Some((cs, ce)) = current {
        total += ce - cs;
    }
    total
}

pub fn summarize(entries: &[TimeEntry], range: Range, now: i64) -> Summary {
    let mut intervals = Vec::new();
    let mut per_project: HashMap<Option<String>, i64> = HashMap::new();
    for entry in entries {
        if let Some((s, e)) = clip(entry, range, now) {
            intervals.push((s, e));
            *per_project.entry(entry.project_id.clone()).or_default() += e - s;
        }
    }
    let entry_count = intervals.len() as u32;
    let mut by_project: Vec<ProjectTotal> = per_project
        .into_iter()
        .map(|(project_id, ms)| ProjectTotal { project_id, ms })
        .collect();
    by_project.sort_by(|a, b| b.ms.cmp(&a.ms).then_with(|| a.project_id.cmp(&b.project_id)));
    Summary {
        range,
        total_ms: union_ms(intervals),
        by_project,
        entry_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntrySource;

    fn entry(project: Option<&str>, start: i64, end: Option<i64>) -> TimeEntry {
        TimeEntry {
            id: format!("{start}"),
            workspace_id: "w".into(),
            user_id: "u".into(),
            activity_id: None,
            project_id: project.map(Into::into),
            start_at: start,
            end_at: end,
            tz: "UTC".into(),
            note: String::new(),
            billable: false,
            source: EntrySource::Manual,
            source_ref: None,
            locked: false,
            tag_ids: vec![],
            cost_center_id: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn union_merges_overlaps_and_touching() {
        assert_eq!(union_ms(vec![(0, 10), (5, 15), (15, 20), (30, 40)]), 30);
        assert_eq!(union_ms(vec![]), 0);
        assert_eq!(union_ms(vec![(0, 100), (10, 20)]), 100);
    }

    #[test]
    fn summarize_clips_to_range_and_counts_running_until_now() {
        let entries = vec![
            entry(Some("a"), -50, Some(50)),  // clipped to 0..50
            entry(Some("b"), 40, Some(60)),   // overlaps a by 10
            entry(None, 90, None),            // running, until now=100
            entry(Some("a"), 200, Some(300)), // outside
        ];
        let s = summarize(&entries, Range { from: 0, to: 150 }, 100);
        assert_eq!(s.total_ms, 70);
        assert_eq!(s.entry_count, 3);
        assert_eq!(
            s.by_project,
            vec![
                ProjectTotal {
                    project_id: Some("a".into()),
                    ms: 50
                },
                ProjectTotal {
                    project_id: Some("b".into()),
                    ms: 20
                },
                ProjectTotal {
                    project_id: None,
                    ms: 10
                },
            ]
        );
    }
}
