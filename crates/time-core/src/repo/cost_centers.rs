//! Cost centers, the user's default cost center over time, and the
//! resolution of an entry's cost center (docs/SPEC.md §4.1).

use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite};

use super::{fetch_project, not_found, soft_delete, unique_violation};
use crate::db::{Core, Stamp, WriteTx};
use crate::model::{CostCenter, CostCenterAssignment, CostCenterAssignmentInput, CostCenterInput};
use crate::{worktime, Error, Result};

macro_rules! columns {
    () => {
        "id, workspace_id, code, name, parent_id, active, valid_from, valid_to, created_at, updated_at"
    };
}

fn from_row(r: &SqliteRow) -> Result<CostCenter> {
    Ok(CostCenter {
        id: r.try_get("id")?,
        workspace_id: r.try_get("workspace_id")?,
        code: r.try_get("code")?,
        name: r.try_get("name")?,
        parent_id: r.try_get("parent_id")?,
        active: r.try_get("active")?,
        valid_from: r.try_get("valid_from")?,
        valid_to: r.try_get("valid_to")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

pub(crate) async fn fetch_cost_center<'e, E>(executor: E, workspace_id: &str, id: &str) -> Result<CostCenter>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let row = sqlx::query(concat!(
        "SELECT ",
        columns!(),
        " FROM cost_center WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL"
    ))
    .bind(id)
    .bind(workspace_id)
    .fetch_optional(executor)
    .await?
    .ok_or_else(|| not_found("cost center", id))?;
    from_row(&row)
}

/// Checks that a cost center referenced from `field` exists.
pub(crate) async fn check_cost_center_ref(tx: &mut WriteTx<'_>, field: &str, id: Option<&str>) -> Result<()> {
    if let Some(id) = id {
        let ws = tx.ctx().workspace_id.clone();
        fetch_cost_center(tx.conn(), &ws, id)
            .await
            .map_err(|_| Error::validation(field, "cost center not found"))?;
    }
    Ok(())
}

/// Cost center for an entry starting at `start_at`: the explicit value,
/// else the project's default, else the user's default on that day.
/// Defaults that are inactive or outside their validity are skipped.
pub(crate) async fn resolve_cost_center(
    tx: &mut WriteTx<'_>,
    explicit: Option<&str>,
    project_id: Option<&str>,
    start_at: i64,
) -> Result<Option<String>> {
    if explicit.is_some() {
        check_cost_center_ref(tx, "costCenterId", explicit).await?;
        return Ok(explicit.map(str::to_string));
    }
    let ws = tx.ctx().workspace_id.clone();
    let user = tx.ctx().user_id.clone();
    let tz = tx.default_tz().parse().unwrap_or(chrono_tz::UTC);
    let date = worktime::local_date(start_at, tz);

    let mut candidates = Vec::new();
    if let Some(pid) = project_id {
        candidates.extend(fetch_project(tx.conn(), &ws, pid).await?.cost_center_id);
    }
    candidates.extend(
        sqlx::query_scalar::<_, String>(
            "SELECT cost_center_id FROM cost_center_assignment
             WHERE user_id = ? AND deleted_at IS NULL
               AND valid_from <= ? AND (valid_to IS NULL OR valid_to >= ?)
             ORDER BY valid_from DESC",
        )
        .bind(&user)
        .bind(date)
        .bind(date)
        .fetch_all(tx.conn())
        .await?,
    );
    for id in candidates {
        if let Ok(cc) = fetch_cost_center(tx.conn(), &ws, &id).await {
            if cc.usable_on(date) {
                return Ok(Some(cc.id));
            }
        }
    }
    Ok(None)
}

const CODE_TAKEN: &str = "is already used by another cost center";

fn assignment_from_row(r: &SqliteRow) -> Result<CostCenterAssignment> {
    Ok(CostCenterAssignment {
        id: r.try_get("id")?,
        cost_center_id: r.try_get("cost_center_id")?,
        valid_from: r.try_get("valid_from")?,
        valid_to: r.try_get("valid_to")?,
    })
}

impl Core {
    pub async fn list_cost_centers(&self) -> Result<Vec<CostCenter>> {
        let rows = sqlx::query(concat!(
            "SELECT ",
            columns!(),
            " FROM cost_center WHERE workspace_id = ? AND deleted_at IS NULL ORDER BY lower(code)"
        ))
        .bind(&self.ctx().workspace_id)
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(from_row).collect()
    }

    pub async fn save_cost_center(&self, id: Option<&str>, input: CostCenterInput) -> Result<CostCenter> {
        let input = input.normalized()?;
        let mut tx = self.begin().await?;
        let ws = tx.ctx().workspace_id.clone();
        let existing = match id {
            Some(id) => Some(fetch_cost_center(tx.conn(), &ws, id).await?),
            None => None,
        };
        let id = existing
            .as_ref()
            .map(|c| c.id.clone())
            .unwrap_or_else(crate::ids::new_id);
        check_parent(&mut tx, &id, input.parent_id.as_deref()).await?;
        let s = tx.stamp();
        let cc = CostCenter {
            id,
            workspace_id: ws,
            code: input.code,
            name: input.name,
            parent_id: input.parent_id,
            active: input.active,
            valid_from: input.valid_from,
            valid_to: input.valid_to,
            created_at: existing.as_ref().map_or(s.now, |c| c.created_at),
            updated_at: s.now,
        };
        write(&mut tx, &cc, &s, existing.is_none()).await?;
        tx.commit().await?;
        Ok(cc)
    }

    /// Cost centers that were used cannot be deleted; deactivate them instead.
    pub async fn delete_cost_center(&self, id: &str) -> Result<()> {
        let mut tx = self.begin().await?;
        let ws = tx.ctx().workspace_id.clone();
        fetch_cost_center(tx.conn(), &ws, id).await?;
        let used: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM time_entry WHERE cost_center_id = ?1 AND deleted_at IS NULL)
                 OR EXISTS (SELECT 1 FROM project WHERE cost_center_id = ?1 AND deleted_at IS NULL)
                 OR EXISTS (SELECT 1 FROM cost_center_assignment WHERE cost_center_id = ?1 AND deleted_at IS NULL)",
        )
        .bind(id)
        .fetch_one(tx.conn())
        .await?;
        if used {
            return Err(Error::Conflict(
                "This cost center is in use. Deactivate it instead of deleting it.".into(),
            ));
        }
        let has_children: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM cost_center WHERE parent_id = ? AND deleted_at IS NULL)",
        )
        .bind(id)
        .fetch_one(tx.conn())
        .await?;
        if has_children {
            return Err(Error::Conflict("This cost center has sub-cost centers.".into()));
        }
        soft_delete(&mut tx, "cost_center", id).await?;
        tx.commit().await
    }

    /// The current user's default cost centers, newest first.
    pub async fn list_cost_center_assignments(&self) -> Result<Vec<CostCenterAssignment>> {
        let rows = sqlx::query(
            "SELECT id, cost_center_id, valid_from, valid_to FROM cost_center_assignment
             WHERE user_id = ? AND deleted_at IS NULL ORDER BY valid_from DESC",
        )
        .bind(&self.ctx().user_id)
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(assignment_from_row).collect()
    }

    pub async fn save_cost_center_assignment(
        &self,
        id: Option<&str>,
        input: CostCenterAssignmentInput,
    ) -> Result<CostCenterAssignment> {
        input.validate()?;
        let mut tx = self.begin().await?;
        let ws = tx.ctx().workspace_id.clone();
        let user = tx.ctx().user_id.clone();
        fetch_cost_center(tx.conn(), &ws, &input.cost_center_id)
            .await
            .map_err(|_| Error::validation("costCenterId", "cost center not found"))?;
        let s = tx.stamp();
        let a = CostCenterAssignment {
            id: id.map(str::to_string).unwrap_or_else(crate::ids::new_id),
            cost_center_id: input.cost_center_id,
            valid_from: input.valid_from,
            valid_to: input.valid_to,
        };
        let result = if id.is_some() {
            sqlx::query(
                "UPDATE cost_center_assignment SET cost_center_id = ?, valid_from = ?, valid_to = ?,
                    updated_at = ?, hlc = ?, origin_device = ?
                 WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
            )
            .bind(&a.cost_center_id)
            .bind(a.valid_from)
            .bind(a.valid_to)
            .bind(s.now)
            .bind(&s.hlc)
            .bind(&s.device)
            .bind(&a.id)
            .bind(&user)
            .execute(tx.conn())
            .await?
        } else {
            sqlx::query(
                "INSERT INTO cost_center_assignment (id, workspace_id, user_id, cost_center_id,
                    valid_from, valid_to, created_at, updated_at, hlc, origin_device)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&a.id)
            .bind(&ws)
            .bind(&user)
            .bind(&a.cost_center_id)
            .bind(a.valid_from)
            .bind(a.valid_to)
            .bind(s.now)
            .bind(s.now)
            .bind(&s.hlc)
            .bind(&s.device)
            .execute(tx.conn())
            .await?
        };
        if result.rows_affected() == 0 {
            return Err(not_found("cost center assignment", &a.id));
        }
        tx.log_upsert("cost_center_assignment", &a.id, &s, &a).await?;
        tx.commit().await?;
        Ok(a)
    }

    pub async fn delete_cost_center_assignment(&self, id: &str) -> Result<()> {
        let mut tx = self.begin().await?;
        let user = tx.ctx().user_id.clone();
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM cost_center_assignment WHERE id = ? AND user_id = ? AND deleted_at IS NULL)",
        )
        .bind(id)
        .bind(&user)
        .fetch_one(tx.conn())
        .await?;
        if !exists {
            return Err(not_found("cost center assignment", id));
        }
        soft_delete(&mut tx, "cost_center_assignment", id).await?;
        tx.commit().await
    }
}

/// The parent must exist and must not be the cost center itself or one of
/// its descendants.
async fn check_parent(tx: &mut WriteTx<'_>, id: &str, parent_id: Option<&str>) -> Result<()> {
    let ws = tx.ctx().workspace_id.clone();
    let mut current = parent_id.map(str::to_string);
    let mut depth = 0;
    while let Some(pid) = current {
        if pid == id || depth > 32 {
            return Err(Error::validation("parentId", "would create a cycle"));
        }
        let parent = fetch_cost_center(tx.conn(), &ws, &pid)
            .await
            .map_err(|_| Error::validation("parentId", "parent cost center not found"))?;
        current = parent.parent_id;
        depth += 1;
    }
    Ok(())
}

async fn write(tx: &mut WriteTx<'_>, c: &CostCenter, s: &Stamp, insert: bool) -> Result<()> {
    let sql = if insert {
        "INSERT INTO cost_center (code, name, parent_id, active, valid_from, valid_to,
            updated_at, hlc, origin_device, id, workspace_id, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    } else {
        "UPDATE cost_center SET code = ?, name = ?, parent_id = ?, active = ?, valid_from = ?,
            valid_to = ?, updated_at = ?, hlc = ?, origin_device = ?
         WHERE id = ? AND workspace_id = ? AND created_at = ?"
    };
    sqlx::query(sql)
        .bind(&c.code)
        .bind(&c.name)
        .bind(&c.parent_id)
        .bind(c.active)
        .bind(c.valid_from)
        .bind(c.valid_to)
        .bind(c.updated_at)
        .bind(&s.hlc)
        .bind(&s.device)
        .bind(&c.id)
        .bind(&c.workspace_id)
        .bind(c.created_at)
        .execute(tx.conn())
        .await
        .map_err(|e| unique_violation(e, "code", CODE_TAKEN))?;
    tx.log_upsert("cost_center", &c.id, s, c).await
}
