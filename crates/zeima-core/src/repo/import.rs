//! Writes entries read by [`crate::import`]: projects and tags are matched
//! by name (case does not matter) and created when missing; an entry that is
//! already there (same times, project and note) is skipped, so importing the
//! same file twice changes nothing.

use std::collections::{BTreeSet, HashMap};

use serde::Serialize;

use crate::db::Core;
use crate::import::{ParsedEntry, ParsedImport, SkippedRow};
use crate::model::{
    ActivityInput, BudgetPeriod, EntryInput, ProjectInput, ProjectStatus, ProjectVisibility, TagInput,
};
use crate::summary::Range;
use crate::Result;

const COLORS: [&str; 10] = [
    "#3b82f6", "#22c55e", "#f59e0b", "#ef4444", "#8b5cf6", "#06b6d4", "#ec4899", "#84cc16", "#f97316",
    "#64748b",
];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    /// Entries that would be added.
    pub new_entries: u32,
    /// Entries already here.
    pub duplicates: u32,
    pub new_projects: Vec<String>,
    pub new_tags: Vec<String>,
    /// Rows of the file that cannot be read.
    pub skipped: Vec<SkippedRow>,
    #[cfg_attr(feature = "specta", specta(type = Option<specta_typescript::Number>))]
    pub first_at: Option<i64>,
    #[cfg_attr(feature = "specta", specta(type = Option<specta_typescript::Number>))]
    pub last_at: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub created_entries: u32,
    pub duplicates: u32,
    pub created_projects: u32,
    pub created_tags: u32,
    /// Rows that were not imported, from the file or from the checks on entries.
    pub skipped: Vec<SkippedRow>,
}

type Key = (i64, i64, Option<String>, String);

impl Core {
    async fn import_lookup(
        &self,
        parsed: &ParsedImport,
    ) -> Result<(HashMap<String, String>, HashMap<String, String>, BTreeSet<Key>)> {
        let projects = self
            .list_projects(true)
            .await?
            .into_iter()
            .map(|p| (p.name.to_lowercase(), p.id))
            .collect();
        let tags = self
            .list_tags()
            .await?
            .into_iter()
            .filter(|t| t.project_id.is_none())
            .map(|t| (t.name.to_lowercase(), t.id))
            .collect();
        let existing = match (
            parsed.entries.iter().map(|e| e.start_at).min(),
            parsed.entries.iter().map(|e| e.end_at).max(),
        ) {
            (Some(from), Some(to)) => self
                .list_entries(Range { from, to })
                .await?
                .into_iter()
                .filter_map(|e| Some((e.start_at, e.end_at?, e.project_id, e.note)))
                .collect(),
            _ => BTreeSet::new(),
        };
        Ok((projects, tags, existing))
    }

    /// What importing would do, without doing it.
    pub async fn preview_import(&self, parsed: &ParsedImport) -> Result<ImportPreview> {
        let (projects, tags, existing) = self.import_lookup(parsed).await?;
        let mut preview = ImportPreview {
            skipped: parsed.skipped.clone(),
            ..Default::default()
        };
        let (mut new_projects, mut new_tags) = (BTreeSet::new(), BTreeSet::new());
        for e in &parsed.entries {
            let project = e
                .project
                .as_ref()
                .and_then(|n| projects.get(&n.to_lowercase()).cloned());
            if existing.contains(&(e.start_at, e.end_at, project, e.note.clone())) {
                preview.duplicates += 1;
                continue;
            }
            preview.new_entries += 1;
            preview.first_at = Some(preview.first_at.map_or(e.start_at, |f| f.min(e.start_at)));
            preview.last_at = Some(preview.last_at.map_or(e.end_at, |l| l.max(e.end_at)));
            if let Some(name) = e
                .project
                .as_ref()
                .filter(|n| !projects.contains_key(&n.to_lowercase()))
            {
                new_projects.insert(name.clone());
            }
            for t in e.tags.iter().filter(|t| !tags.contains_key(&t.to_lowercase())) {
                new_tags.insert(t.clone());
            }
        }
        preview.new_projects = new_projects.into_iter().collect();
        preview.new_tags = new_tags.into_iter().collect();
        Ok(preview)
    }

    /// Adds the entries, with their projects and tags. Entries the usual
    /// checks refuse (a locked day, say) are reported, not fatal.
    pub async fn import_entries(&self, parsed: &ParsedImport) -> Result<ImportReport> {
        let (mut projects, mut tags, existing) = self.import_lookup(parsed).await?;
        let mut report = ImportReport {
            skipped: parsed.skipped.clone(),
            ..Default::default()
        };
        let activity = match self.list_activities(false).await?.into_iter().next() {
            Some(a) => a.id,
            None => {
                self.create_activity(ActivityInput {
                    name: "Work".into(),
                    color: COLORS[0].into(),
                    archived: false,
                })
                .await?
                .id
            }
        };
        let tz = self.default_tz();
        for (i, e) in parsed.entries.iter().enumerate() {
            let project = e
                .project
                .as_ref()
                .and_then(|n| projects.get(&n.to_lowercase()).cloned());
            if existing.contains(&(e.start_at, e.end_at, project.clone(), e.note.clone())) {
                report.duplicates += 1;
                continue;
            }
            let project_id = match (&project, &e.project) {
                (Some(id), _) => Some(id.clone()),
                (None, Some(name)) => {
                    let created = self.create_project(new_project(name, e, projects.len())).await?;
                    projects.insert(name.to_lowercase(), created.id.clone());
                    report.created_projects += 1;
                    Some(created.id)
                }
                (None, None) => None,
            };
            let mut tag_ids = Vec::new();
            for name in &e.tags {
                let id = match tags.get(&name.to_lowercase()) {
                    Some(id) => id.clone(),
                    None => {
                        let created = self
                            .create_tag(TagInput {
                                name: name.clone(),
                                color: COLORS[tags.len() % COLORS.len()].into(),
                                project_id: None,
                                group_name: None,
                            })
                            .await?;
                        tags.insert(name.to_lowercase(), created.id.clone());
                        report.created_tags += 1;
                        created.id
                    }
                };
                tag_ids.push(id);
            }
            let input = EntryInput {
                activity_id: Some(activity.clone()),
                project_id,
                start_at: e.start_at,
                end_at: Some(e.end_at),
                tz: Some(tz.clone()),
                note: e.note.clone(),
                billable: Some(e.billable),
                tag_ids,
                cost_center_id: None,
            };
            match self.create_entry(input).await {
                Ok(_) => report.created_entries += 1,
                Err(err) => report.skipped.push(SkippedRow {
                    line: u32::try_from(i + 2).unwrap_or(u32::MAX),
                    reason: err.to_string(),
                }),
            }
        }
        Ok(report)
    }
}

fn new_project(name: &str, entry: &ParsedEntry, existing: usize) -> ProjectInput {
    ProjectInput {
        name: name.to_string(),
        alias: None,
        color: COLORS[existing % COLORS.len()].into(),
        parent_id: None,
        client: entry.client.clone(),
        budget_minutes: None,
        budget_period: BudgetPeriod::Total,
        budget_warn_pct: 80,
        billable_default: entry.billable,
        status: ProjectStatus::Active,
        cost_center_id: None,
        visibility: ProjectVisibility::Org,
        manager_id: None,
        member_ids: Vec::new(),
    }
}
