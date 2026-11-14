//! Where people work on a day (SPEC §5.11), in shared workspaces.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::db::Core;
use crate::{Error, Result};

crate::model::string_enum!(LocationKind { Office => "office", Home => "home", Travel => "travel", Other => "other" });

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct WorkLocation {
    pub id: String,
    /// The person (their account id).
    pub user_id: String,
    pub date: NaiveDate,
    pub kind: LocationKind,
}

/// The same row for the same person and day on every device.
fn location_id(workspace_id: &str, user_id: &str, date: NaiveDate) -> String {
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!("work_location|{workspace_id}|{user_id}|{date}").as_bytes(),
    )
    .to_string()
}

impl Core {
    /// Locations in `from..=to` that this device has: the person's own and
    /// those of colleagues the server sent.
    pub async fn list_locations(&self, from: NaiveDate, to: NaiveDate) -> Result<Vec<WorkLocation>> {
        let rows = sqlx::query(
            "SELECT id, user_id, date, kind FROM work_location
             WHERE workspace_id = ? AND deleted_at IS NULL AND date >= ? AND date <= ? ORDER BY date, user_id",
        )
        .bind(&self.ctx().workspace_id)
        .bind(from)
        .bind(to)
        .fetch_all(self.pool())
        .await?;
        rows.iter()
            .map(|r| {
                Ok(WorkLocation {
                    id: r.try_get("id")?,
                    user_id: r.try_get("user_id")?,
                    date: r.try_get("date")?,
                    kind: r.try_get::<String, _>("kind")?.parse()?,
                })
            })
            .collect()
    }

    /// Sets (or, with `None`, clears) where the person works on `date`.
    pub async fn set_location(&self, date: NaiveDate, kind: Option<LocationKind>) -> Result<()> {
        if !self.ctx().shared {
            return Err(Error::validation(
                "workspace",
                "work locations are shared in an organization's workspace",
            ));
        }
        let mut tx = self.begin().await?;
        let ws = tx.ctx().workspace_id.clone();
        let user = tx.ctx().user_id.clone();
        let id = location_id(&ws, &user, date);
        let s = tx.stamp();
        match kind {
            Some(kind) => {
                sqlx::query(
                    "INSERT INTO work_location (id, workspace_id, user_id, date, kind, created_at, updated_at, hlc, origin_device)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                     ON CONFLICT (id) DO UPDATE SET kind = excluded.kind, deleted_at = NULL,
                        updated_at = excluded.updated_at, hlc = excluded.hlc, origin_device = excluded.origin_device",
                )
                .bind(&id)
                .bind(&ws)
                .bind(&user)
                .bind(date)
                .bind(kind.as_str())
                .bind(s.now)
                .bind(s.now)
                .bind(&s.hlc)
                .bind(&s.device)
                .execute(tx.conn())
                .await?;
                let row = WorkLocation {
                    id: id.clone(),
                    user_id: user,
                    date,
                    kind,
                };
                tx.log_upsert("work_location", &id, &s, &row).await?;
            }
            None => {
                let exists: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM work_location WHERE id = ? AND deleted_at IS NULL)",
                )
                .bind(&id)
                .fetch_one(tx.conn())
                .await?;
                if exists {
                    super::soft_delete(&mut tx, "work_location", &id).await?;
                }
            }
        }
        tx.commit().await
    }
}
