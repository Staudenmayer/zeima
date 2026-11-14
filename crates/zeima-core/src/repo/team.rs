//! Time tracked by the people of a shared workspace (SPEC §5.9, manager
//! dashboards): what this device has received of their entries, which the
//! server limits to what the person's role may see.

use std::collections::BTreeMap;

use serde::Serialize;
use sqlx::Row;

use crate::db::Core;
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
            });
            member.total_ms += ms;
            member.billable_ms += row.try_get::<i64, _>("billable_ms")?;
            member.projects.push(ProjectTotal {
                project_id: row.try_get("project_id")?,
                ms,
            });
        }
        let mut list: Vec<TeamMember> = members.into_values().collect();
        for m in &mut list {
            m.projects.sort_by_key(|p| std::cmp::Reverse(p.ms));
        }
        list.sort_by(|a, b| {
            b.total_ms
                .cmp(&a.total_ms)
                .then_with(|| a.user_id.cmp(&b.user_id))
        });
        Ok(list)
    }
}
