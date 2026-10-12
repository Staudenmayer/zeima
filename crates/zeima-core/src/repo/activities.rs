use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite};

use super::{not_found, soft_delete, unique_violation};
use crate::db::{Core, Stamp, WriteTx};
use crate::model::{Activity, ActivityInput, RecentActivity};
use crate::{Error, Result};

macro_rules! columns {
    () => {
        "id, workspace_id, name, color, archived, created_at, updated_at"
    };
}

fn from_row(r: &SqliteRow) -> Result<Activity> {
    Ok(Activity {
        id: r.try_get("id")?,
        workspace_id: r.try_get("workspace_id")?,
        name: r.try_get("name")?,
        color: r.try_get("color")?,
        archived: r.try_get("archived")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

const NAME_TAKEN: &str = "an activity with this name already exists";

pub(crate) async fn fetch_activity<'e, E>(executor: E, workspace_id: &str, id: &str) -> Result<Activity>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let row = sqlx::query(concat!(
        "SELECT ",
        columns!(),
        " FROM activity WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL"
    ))
    .bind(id)
    .bind(workspace_id)
    .fetch_optional(executor)
    .await?
    .ok_or_else(|| not_found("activity", id))?;
    from_row(&row)
}

/// The activity of a new or edited entry: required, and it must exist.
/// Archived activities are rejected unless the entry already had it.
pub(crate) async fn check_entry_activity(
    tx: &mut WriteTx<'_>,
    activity_id: Option<&str>,
    previous: Option<&str>,
) -> Result<()> {
    let Some(id) = activity_id else {
        return Err(Error::validation("activityId", "choose an activity"));
    };
    let ws = tx.ctx().workspace_id.clone();
    let activity = fetch_activity(tx.conn(), &ws, id)
        .await
        .map_err(|_| Error::validation("activityId", "activity not found"))?;
    if activity.archived && previous != Some(id) {
        return Err(Error::validation("activityId", "this activity is archived"));
    }
    Ok(())
}

impl Core {
    /// Activities by name; archived ones only when asked for.
    pub async fn list_activities(&self, include_archived: bool) -> Result<Vec<Activity>> {
        let rows = sqlx::query(concat!(
            "SELECT ",
            columns!(),
            " FROM activity WHERE workspace_id = ? AND deleted_at IS NULL AND (? OR archived = 0)
             ORDER BY lower(name)"
        ))
        .bind(&self.ctx().workspace_id)
        .bind(include_archived)
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(from_row).collect()
    }

    pub async fn create_activity(&self, input: ActivityInput) -> Result<Activity> {
        let input = input.normalized()?;
        let mut tx = self.begin().await?;
        let s = tx.stamp();
        let activity = Activity {
            id: crate::ids::new_id(),
            workspace_id: tx.ctx().workspace_id.clone(),
            name: input.name,
            color: input.color,
            archived: input.archived,
            created_at: s.now,
            updated_at: s.now,
        };
        write(&mut tx, &activity, &s, true).await?;
        tx.commit().await?;
        Ok(activity)
    }

    pub async fn update_activity(&self, id: &str, input: ActivityInput) -> Result<Activity> {
        let input = input.normalized()?;
        let mut tx = self.begin().await?;
        let ws = tx.ctx().workspace_id.clone();
        let existing = fetch_activity(tx.conn(), &ws, id).await?;
        let s = tx.stamp();
        let activity = Activity {
            name: input.name,
            color: input.color,
            archived: input.archived,
            updated_at: s.now,
            ..existing
        };
        write(&mut tx, &activity, &s, false).await?;
        tx.commit().await?;
        Ok(activity)
    }

    /// Soft-deletes an activity. Activities with time entries cannot be
    /// deleted; archive them instead.
    pub async fn delete_activity(&self, id: &str) -> Result<()> {
        let mut tx = self.begin().await?;
        let ws = tx.ctx().workspace_id.clone();
        fetch_activity(tx.conn(), &ws, id).await?;
        let has_entries: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM time_entry WHERE activity_id = ? AND deleted_at IS NULL)",
        )
        .bind(id)
        .fetch_one(tx.conn())
        .await?;
        if has_entries {
            return Err(Error::Conflict(
                "This activity has time entries. Archive it instead of deleting it.".into(),
            ));
        }
        soft_delete(&mut tx, "activity", id).await?;
        tx.commit().await
    }

    /// Most recently tracked activity/project/tags combinations, newest
    /// first, skipping archived or deleted activities and projects.
    pub async fn recent_activities(&self, limit: u32) -> Result<Vec<RecentActivity>> {
        // Scan a bounded window of recent entries; combinations repeat a lot.
        let rows: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT e.activity_id, e.project_id,
                    (SELECT group_concat(et.tag_id) FROM time_entry_tag et
                       JOIN tag t ON t.id = et.tag_id AND t.deleted_at IS NULL
                      WHERE et.entry_id = e.id) AS tag_ids
               FROM time_entry e
               JOIN activity a ON a.id = e.activity_id AND a.deleted_at IS NULL AND a.archived = 0
               LEFT JOIN project p ON p.id = e.project_id
              WHERE e.user_id = ? AND e.deleted_at IS NULL
                AND (e.project_id IS NULL OR (p.deleted_at IS NULL AND p.status != 'archived'))
              ORDER BY e.start_at DESC
              LIMIT 500",
        )
        .bind(&self.ctx().user_id)
        .fetch_all(self.pool())
        .await?;
        let mut out: Vec<RecentActivity> = Vec::new();
        for (activity_id, project_id, tag_ids) in rows {
            let mut tag_ids: Vec<String> = tag_ids
                .map(|s| s.split(',').map(str::to_string).collect())
                .unwrap_or_default();
            tag_ids.sort();
            let item = RecentActivity {
                activity_id,
                project_id,
                tag_ids,
            };
            if !out.contains(&item) {
                out.push(item);
                if out.len() >= limit as usize {
                    break;
                }
            }
        }
        Ok(out)
    }
}

async fn write(tx: &mut WriteTx<'_>, a: &Activity, s: &Stamp, insert: bool) -> Result<()> {
    let sql = if insert {
        "INSERT INTO activity (name, color, archived, updated_at, hlc, origin_device,
            id, workspace_id, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
    } else {
        "UPDATE activity SET name = ?, color = ?, archived = ?, updated_at = ?, hlc = ?, origin_device = ?
         WHERE id = ? AND workspace_id = ? AND created_at = ?"
    };
    sqlx::query(sql)
        .bind(&a.name)
        .bind(&a.color)
        .bind(a.archived)
        .bind(a.updated_at)
        .bind(&s.hlc)
        .bind(&s.device)
        .bind(&a.id)
        .bind(&a.workspace_id)
        .bind(a.created_at)
        .execute(tx.conn())
        .await
        .map_err(|e| unique_violation(e, "name", NAME_TAKEN))?;
    tx.log_upsert("activity", &a.id, s, a).await
}
