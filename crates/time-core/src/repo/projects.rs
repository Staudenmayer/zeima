use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite};

use super::{check_cost_center_ref, not_found, unique_violation};
use crate::db::{Core, Stamp, WriteTx};
use crate::model::{Project, ProjectInput, ProjectStatus};
use crate::{Error, Result};

macro_rules! columns {
    () => {
        "id, workspace_id, name, alias, color, parent_id, client, budget_minutes, \
         budget_period, budget_warn_pct, billable_default, status, cost_center_id, created_at, updated_at"
    };
}

fn from_row(r: &SqliteRow) -> Result<Project> {
    Ok(Project {
        id: r.try_get("id")?,
        workspace_id: r.try_get("workspace_id")?,
        name: r.try_get("name")?,
        alias: r.try_get("alias")?,
        color: r.try_get("color")?,
        parent_id: r.try_get("parent_id")?,
        client: r.try_get("client")?,
        budget_minutes: r.try_get("budget_minutes")?,
        budget_period: r.try_get::<String, _>("budget_period")?.parse()?,
        budget_warn_pct: r.try_get("budget_warn_pct")?,
        billable_default: r.try_get("billable_default")?,
        status: r.try_get::<String, _>("status")?.parse()?,
        cost_center_id: r.try_get("cost_center_id")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

pub(crate) async fn fetch_project<'e, E>(executor: E, workspace_id: &str, id: &str) -> Result<Project>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let row = sqlx::query(concat!(
        "SELECT ",
        columns!(),
        " FROM project WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL"
    ))
    .bind(id)
    .bind(workspace_id)
    .fetch_optional(executor)
    .await?
    .ok_or_else(|| not_found("project", id))?;
    from_row(&row)
}

const ALIAS_TAKEN: &str = "is already used by another project";

impl Core {
    pub async fn list_projects(&self, include_archived: bool) -> Result<Vec<Project>> {
        let rows = sqlx::query(concat!(
            "SELECT ",
            columns!(),
            " FROM project
             WHERE workspace_id = ? AND deleted_at IS NULL AND (? OR status != 'archived')
             ORDER BY status = 'archived', lower(name)"
        ))
        .bind(&self.ctx().workspace_id)
        .bind(include_archived)
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(from_row).collect()
    }

    pub async fn get_project(&self, id: &str) -> Result<Project> {
        fetch_project(self.pool(), &self.ctx().workspace_id, id).await
    }

    pub async fn create_project(&self, input: ProjectInput) -> Result<Project> {
        let input = input.normalized()?;
        let mut tx = self.begin().await?;
        let id = crate::ids::new_id();
        check_parent(&mut tx, &id, input.parent_id.as_deref()).await?;
        check_cost_center_ref(&mut tx, "costCenterId", input.cost_center_id.as_deref()).await?;
        let s = tx.stamp();
        let project = Project {
            id,
            workspace_id: tx.ctx().workspace_id.clone(),
            created_at: s.now,
            updated_at: s.now,
            ..from_input(input)
        };
        write(&mut tx, &project, &s, true).await?;
        tx.commit().await?;
        Ok(project)
    }

    pub async fn update_project(&self, id: &str, input: ProjectInput) -> Result<Project> {
        let input = input.normalized()?;
        let mut tx = self.begin().await?;
        let ws = tx.ctx().workspace_id.clone();
        let existing = fetch_project(&mut *tx.conn(), &ws, id).await?;
        check_parent(&mut tx, id, input.parent_id.as_deref()).await?;
        check_cost_center_ref(&mut tx, "costCenterId", input.cost_center_id.as_deref()).await?;
        let s = tx.stamp();
        let project = Project {
            id: existing.id,
            workspace_id: existing.workspace_id,
            created_at: existing.created_at,
            updated_at: s.now,
            ..from_input(input)
        };
        write(&mut tx, &project, &s, false).await?;
        tx.commit().await?;
        Ok(project)
    }

    /// Soft-deletes a project and its project-scoped tags. Projects with
    /// time entries cannot be deleted; archive them instead.
    pub async fn delete_project(&self, id: &str) -> Result<()> {
        let mut tx = self.begin().await?;
        let ws = tx.ctx().workspace_id.clone();
        fetch_project(&mut *tx.conn(), &ws, id).await?;
        let has_entries: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM time_entry WHERE project_id = ? AND deleted_at IS NULL)",
        )
        .bind(id)
        .fetch_one(&mut *tx.conn())
        .await?;
        if has_entries {
            return Err(Error::Conflict(
                "This project has time entries. Archive it instead of deleting it.".into(),
            ));
        }
        let has_children: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM project WHERE parent_id = ? AND deleted_at IS NULL)",
        )
        .bind(id)
        .fetch_one(&mut *tx.conn())
        .await?;
        if has_children {
            return Err(Error::Conflict("This project has sub-projects.".into()));
        }

        let tag_ids: Vec<String> =
            sqlx::query_scalar("SELECT id FROM tag WHERE project_id = ? AND deleted_at IS NULL")
                .bind(id)
                .fetch_all(&mut *tx.conn())
                .await?;
        for tag_id in tag_ids {
            soft_delete(&mut tx, "tag", &tag_id).await?;
        }
        soft_delete(&mut tx, "project", id).await?;
        tx.commit().await
    }
}

/// Tables that support soft delete. The table name is interpolated into
/// SQL, so it must come from this fixed list.
const SOFT_DELETE_TABLES: &[&str] = &[
    "project",
    "tag",
    "time_entry",
    "pause",
    "work_schedule",
    "absence_type",
    "absence",
    "custom_holiday",
    "balance_adjustment",
    "cost_center",
    "cost_center_assignment",
];

pub(crate) async fn soft_delete(tx: &mut WriteTx<'_>, table: &'static str, id: &str) -> Result<()> {
    assert!(
        SOFT_DELETE_TABLES.contains(&table),
        "soft_delete on unknown table {table}"
    );
    let s = tx.stamp();
    let sql =
        format!("UPDATE {table} SET deleted_at = ?, updated_at = ?, hlc = ?, origin_device = ? WHERE id = ?");
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(s.now)
        .bind(s.now)
        .bind(&s.hlc)
        .bind(&s.device)
        .bind(id)
        .execute(tx.conn())
        .await?;
    if table == "time_entry" {
        tx.emit(crate::events::DomainEvent::EntryDeleted {
            entry_id: id.to_string(),
        });
    }
    tx.log_delete(table, id, &s).await
}

fn from_input(input: ProjectInput) -> Project {
    Project {
        id: String::new(),
        workspace_id: String::new(),
        name: input.name,
        alias: input.alias,
        color: input.color,
        parent_id: input.parent_id,
        client: input.client,
        budget_minutes: input.budget_minutes,
        budget_period: input.budget_period,
        budget_warn_pct: input.budget_warn_pct,
        billable_default: input.billable_default,
        status: input.status,
        cost_center_id: input.cost_center_id,
        created_at: 0,
        updated_at: 0,
    }
}

/// The parent must exist and must not be the project itself or one of its
/// descendants.
async fn check_parent(tx: &mut WriteTx<'_>, id: &str, parent_id: Option<&str>) -> Result<()> {
    let ws = tx.ctx().workspace_id.clone();
    let mut current = parent_id.map(str::to_string);
    let mut depth = 0;
    while let Some(pid) = current {
        if pid == id || depth > 32 {
            return Err(Error::validation("parentId", "would create a cycle"));
        }
        let parent = fetch_project(&mut *tx.conn(), &ws, &pid)
            .await
            .map_err(|_| Error::validation("parentId", "parent project not found"))?;
        current = parent.parent_id;
        depth += 1;
    }
    Ok(())
}

async fn write(tx: &mut WriteTx<'_>, p: &Project, s: &Stamp, insert: bool) -> Result<()> {
    let sql = if insert {
        "INSERT INTO project (name, alias, color, parent_id, client, budget_minutes, budget_period,
            budget_warn_pct, billable_default, status, cost_center_id, updated_at, hlc, origin_device,
            id, workspace_id, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    } else {
        "UPDATE project SET name = ?, alias = ?, color = ?, parent_id = ?, client = ?,
            budget_minutes = ?, budget_period = ?, budget_warn_pct = ?, billable_default = ?,
            status = ?, cost_center_id = ?, updated_at = ?, hlc = ?, origin_device = ?
         WHERE id = ? AND workspace_id = ? AND created_at = ?"
    };
    sqlx::query(sql)
        .bind(&p.name)
        .bind(&p.alias)
        .bind(&p.color)
        .bind(&p.parent_id)
        .bind(&p.client)
        .bind(p.budget_minutes)
        .bind(p.budget_period.as_str())
        .bind(p.budget_warn_pct)
        .bind(p.billable_default)
        .bind(p.status.as_str())
        .bind(&p.cost_center_id)
        .bind(p.updated_at)
        .bind(&s.hlc)
        .bind(&s.device)
        .bind(&p.id)
        .bind(&p.workspace_id)
        .bind(p.created_at)
        .execute(tx.conn())
        .await
        .map_err(|e| unique_violation(e, "alias", ALIAS_TAKEN))?;
    tx.log_upsert("project", &p.id, s, p).await
}

/// Archived projects cannot receive new time.
pub(crate) fn ensure_trackable(project: &Project) -> Result<()> {
    if project.status == ProjectStatus::Archived {
        return Err(Error::validation("projectId", "project is archived"));
    }
    Ok(())
}
