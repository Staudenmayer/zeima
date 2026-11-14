//! Time tracked by the people of a shared workspace (SPEC §5.9, manager
//! dashboards): what this device has received of their entries, which the
//! server limits to what the person's role may see.

use std::collections::BTreeMap;

use serde::Serialize;
use sqlx::Row;

use crate::db::Core;
use crate::model::TimeEntry;
use crate::summary::{ProjectTotal, Range};
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct TeamMember {
    /// Account id.
    pub user_id: String,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub total_ms: i64,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub billable_ms: i64,
    /// Per project, longest first.
    pub projects: Vec<ProjectTotal>,
    /// Per tag (an entry with two tags counts for both), longest first.
    pub tags: Vec<IdTotal>,
    /// Per cost center, longest first.
    pub cost_centers: Vec<IdTotal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct IdTotal {
    pub id: String,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub ms: i64,
}

impl Core {
    /// Finished entries of everybody in this workspace within `range`, as
    /// time per person and project (entries are clipped to the range).
    /// Running entries are not included.
    pub async fn team_summary(&self, range: Range) -> Result<Vec<TeamMember>> {
        let rows = sqlx::query(
            "SELECT user_id, project_id,
                    SUM(MIN(end_at, ?2) - MAX(start_at, ?1)) AS ms,
                    SUM(CASE WHEN billable THEN MIN(end_at, ?2) - MAX(start_at, ?1) ELSE 0 END) AS billable_ms
             FROM time_entry
             WHERE workspace_id = ?3 AND deleted_at IS NULL AND end_at IS NOT NULL
               AND start_at < ?2 AND end_at > ?1
             GROUP BY user_id, project_id",
        )
        .bind(range.from)
        .bind(range.to)
        .bind(&self.ctx().workspace_id)
        .fetch_all(self.pool())
        .await?;
        let mut members: BTreeMap<String, TeamMember> = BTreeMap::new();
        for row in &rows {
            let user: String = row.try_get("user_id")?;
            let ms: i64 = row.try_get("ms")?;
            let member = members.entry(user.clone()).or_insert_with(|| TeamMember {
                user_id: user,
                total_ms: 0,
                billable_ms: 0,
                projects: Vec::new(),
                tags: Vec::new(),
                cost_centers: Vec::new(),
            });
            member.total_ms += ms;
            member.billable_ms += row.try_get::<i64, _>("billable_ms")?;
            member.projects.push(ProjectTotal {
                project_id: row.try_get("project_id")?,
                ms,
            });
        }
        // Tags and cost centers, per person.
        let tag_rows = sqlx::query(
            "SELECT te.user_id AS user_id, tt.tag_id AS tag_id, SUM(MIN(te.end_at, ?2) - MAX(te.start_at, ?1)) AS ms
             FROM time_entry te JOIN time_entry_tag tt ON tt.entry_id = te.id
             WHERE te.workspace_id = ?3 AND te.deleted_at IS NULL AND te.end_at IS NOT NULL
               AND te.start_at < ?2 AND te.end_at > ?1
             GROUP BY te.user_id, tt.tag_id",
        )
        .bind(range.from)
        .bind(range.to)
        .bind(&self.ctx().workspace_id)
        .fetch_all(self.pool())
        .await?;
        for row in &tag_rows {
            if let Some(m) = members.get_mut(&row.try_get::<String, _>("user_id")?) {
                m.tags.push(IdTotal {
                    id: row.try_get("tag_id")?,
                    ms: row.try_get("ms")?,
                });
            }
        }
        let cc_rows = sqlx::query(
            "SELECT user_id, cost_center_id, SUM(MIN(end_at, ?2) - MAX(start_at, ?1)) AS ms
             FROM time_entry
             WHERE workspace_id = ?3 AND deleted_at IS NULL AND end_at IS NOT NULL AND cost_center_id IS NOT NULL
               AND start_at < ?2 AND end_at > ?1
             GROUP BY user_id, cost_center_id",
        )
        .bind(range.from)
        .bind(range.to)
        .bind(&self.ctx().workspace_id)
        .fetch_all(self.pool())
        .await?;
        for row in &cc_rows {
            if let Some(m) = members.get_mut(&row.try_get::<String, _>("user_id")?) {
                m.cost_centers.push(IdTotal {
                    id: row.try_get("cost_center_id")?,
                    ms: row.try_get("ms")?,
                });
            }
        }
        let mut list: Vec<TeamMember> = members.into_values().collect();
        for m in &mut list {
            m.projects.sort_by_key(|p| std::cmp::Reverse(p.ms));
            m.tags.sort_by_key(|t| std::cmp::Reverse(t.ms));
            m.cost_centers.sort_by_key(|c| std::cmp::Reverse(c.ms));
        }
        list.sort_by(|a, b| {
            b.total_ms
                .cmp(&a.total_ms)
                .then_with(|| a.user_id.cmp(&b.user_id))
        });
        Ok(list)
    }

    /// The finished entries one person has in this workspace within `range`,
    /// newest first (what the server sent this device of them).
    pub async fn team_entries(&self, user_id: &str, range: Range) -> Result<Vec<TimeEntry>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM time_entry
             WHERE workspace_id = ? AND user_id = ? AND deleted_at IS NULL AND end_at IS NOT NULL
               AND start_at < ? AND end_at > ? ORDER BY start_at DESC LIMIT 500",
        )
        .bind(&self.ctx().workspace_id)
        .bind(user_id)
        .bind(range.to)
        .bind(range.from)
        .fetch_all(self.pool())
        .await?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            out.push(super::fetch_entry(self.pool(), user_id, &id).await?);
        }
        Ok(out)
    }
}
