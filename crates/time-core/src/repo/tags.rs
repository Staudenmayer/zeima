use std::collections::HashSet;

use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

use super::{fetch_project, not_found, soft_delete, unique_violation};
use crate::db::{Core, Stamp, WriteTx};
use crate::model::{Tag, TagInput};
use crate::{Error, Result};

macro_rules! columns {
    () => {
        "id, workspace_id, name, color, project_id, group_name, created_at, updated_at"
    };
}

fn from_row(r: &SqliteRow) -> Result<Tag> {
    Ok(Tag {
        id: r.try_get("id")?,
        workspace_id: r.try_get("workspace_id")?,
        name: r.try_get("name")?,
        color: r.try_get("color")?,
        project_id: r.try_get("project_id")?,
        group_name: r.try_get("group_name")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

const NAME_TAKEN: &str = "a tag with this name already exists here";

impl Core {
    pub async fn list_tags(&self) -> Result<Vec<Tag>> {
        let rows = sqlx::query(concat!(
            "SELECT ",
            columns!(),
            " FROM tag WHERE workspace_id = ? AND deleted_at IS NULL
             ORDER BY project_id IS NOT NULL, coalesce(group_name, ''), lower(name)"
        ))
        .bind(&self.ctx().workspace_id)
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(from_row).collect()
    }

    pub async fn create_tag(&self, input: TagInput) -> Result<Tag> {
        let input = input.normalized()?;
        let mut tx = self.begin().await?;
        check_project(&mut tx, input.project_id.as_deref()).await?;
        let s = tx.stamp();
        let tag = Tag {
            id: crate::ids::new_id(),
            workspace_id: tx.ctx().workspace_id.clone(),
            name: input.name,
            color: input.color,
            project_id: input.project_id,
            group_name: input.group_name,
            created_at: s.now,
            updated_at: s.now,
        };
        write(&mut tx, &tag, &s, true).await?;
        tx.commit().await?;
        Ok(tag)
    }

    pub async fn update_tag(&self, id: &str, input: TagInput) -> Result<Tag> {
        let input = input.normalized()?;
        let mut tx = self.begin().await?;
        let existing = fetch_tag(tx.conn(), id).await?;
        if existing.project_id != input.project_id {
            return Err(Error::validation(
                "projectId",
                "the scope of a tag cannot be changed",
            ));
        }
        let s = tx.stamp();
        let tag = Tag {
            name: input.name,
            color: input.color,
            group_name: input.group_name,
            updated_at: s.now,
            ..existing
        };
        write(&mut tx, &tag, &s, false).await?;
        tx.commit().await?;
        Ok(tag)
    }

    /// Soft-deletes a tag. Entries keep the link but no longer report it.
    pub async fn delete_tag(&self, id: &str) -> Result<()> {
        let mut tx = self.begin().await?;
        fetch_tag(tx.conn(), id).await?;
        soft_delete(&mut tx, "tag", id).await?;
        tx.commit().await
    }
}

async fn fetch_tag(conn: &mut SqliteConnection, id: &str) -> Result<Tag> {
    let row = sqlx::query(concat!(
        "SELECT ",
        columns!(),
        " FROM tag WHERE id = ? AND deleted_at IS NULL"
    ))
    .bind(id)
    .fetch_optional(conn)
    .await?
    .ok_or_else(|| not_found("tag", id))?;
    from_row(&row)
}

async fn check_project(tx: &mut WriteTx<'_>, project_id: Option<&str>) -> Result<()> {
    if let Some(pid) = project_id {
        let ws = tx.ctx().workspace_id.clone();
        fetch_project(tx.conn(), &ws, pid)
            .await
            .map_err(|_| Error::validation("projectId", "project not found"))?;
    }
    Ok(())
}

async fn write(tx: &mut WriteTx<'_>, t: &Tag, s: &Stamp, insert: bool) -> Result<()> {
    let sql = if insert {
        "INSERT INTO tag (name, color, group_name, updated_at, hlc, origin_device,
            id, workspace_id, project_id, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    } else {
        "UPDATE tag SET name = ?, color = ?, group_name = ?, updated_at = ?, hlc = ?, origin_device = ?
         WHERE id = ? AND workspace_id = ? AND project_id IS ? AND created_at = ?"
    };
    sqlx::query(sql)
        .bind(&t.name)
        .bind(&t.color)
        .bind(&t.group_name)
        .bind(t.updated_at)
        .bind(&s.hlc)
        .bind(&s.device)
        .bind(&t.id)
        .bind(&t.workspace_id)
        .bind(&t.project_id)
        .bind(t.created_at)
        .execute(tx.conn())
        .await
        .map_err(|e| unique_violation(e, "name", NAME_TAKEN))?;
    tx.log_upsert("tag", &t.id, s, t).await
}

/// Tags on an entry must exist, be global or belong to the entry's project,
/// and at most one tag per tag group may be used.
pub(crate) async fn validate_entry_tags(
    conn: &mut SqliteConnection,
    workspace_id: &str,
    project_id: Option<&str>,
    tag_ids: &[String],
) -> Result<()> {
    let mut groups = HashSet::new();
    for id in tag_ids {
        let tag = fetch_tag(&mut *conn, id)
            .await
            .map_err(|_| Error::validation("tagIds", format!("unknown tag {id}")))?;
        if tag.workspace_id != workspace_id {
            return Err(Error::validation("tagIds", format!("unknown tag {id}")));
        }
        if tag.project_id.is_some() && tag.project_id.as_deref() != project_id {
            return Err(Error::validation(
                "tagIds",
                format!("tag '{}' belongs to another project", tag.name),
            ));
        }
        if let Some(group) = tag.group_name {
            if !groups.insert(group.clone()) {
                return Err(Error::validation(
                    "tagIds",
                    format!("only one tag of group '{group}' can be used"),
                ));
            }
        }
    }
    Ok(())
}
