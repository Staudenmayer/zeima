//! Rooms meetings are held in (SPEC §5.12): a list the organization keeps, so
//! a meeting names a room instead of free text and no room is booked twice.

use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::db::Core;
use crate::{Error, Result};

/// A place for meetings, with room for `capacity` people.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Room {
    pub id: String,
    pub name: String,
    #[cfg_attr(feature = "specta", specta(type = Option<specta_typescript::Number>))]
    pub capacity: Option<i64>,
}

impl Core {
    pub async fn list_rooms(&self) -> Result<Vec<Room>> {
        let rows = sqlx::query("SELECT id, name, capacity FROM room WHERE workspace_id = ? AND deleted_at IS NULL ORDER BY lower(name)")
            .bind(&self.ctx().workspace_id)
            .fetch_all(self.pool())
            .await?;
        rows.iter()
            .map(|r| {
                Ok(Room {
                    id: r.try_get("id")?,
                    name: r.try_get("name")?,
                    capacity: r.try_get("capacity")?,
                })
            })
            .collect()
    }

    pub async fn save_room(&self, id: Option<&str>, name: &str, capacity: Option<i64>) -> Result<Room> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 100 {
            return Err(Error::validation("name", "must be 1 to 100 characters"));
        }
        if capacity.is_some_and(|c| c < 1) {
            return Err(Error::validation("capacity", "must be at least 1"));
        }
        let mut tx = self.begin().await?;
        let ws = tx.ctx().workspace_id.clone();
        let taken: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM room WHERE workspace_id = ? AND lower(name) = lower(?) AND deleted_at IS NULL AND id IS NOT ?)",
        )
        .bind(&ws)
        .bind(name)
        .bind(id)
        .fetch_one(tx.conn())
        .await?;
        if taken {
            return Err(Error::Conflict("another room has this name".into()));
        }
        let id = match id {
            Some(id) => {
                let known: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM room WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL)")
                    .bind(id)
                    .bind(&ws)
                    .fetch_one(tx.conn())
                    .await?;
                if !known {
                    return Err(super::not_found("room", id));
                }
                id.to_string()
            }
            None => crate::ids::new_id(),
        };
        let s = tx.stamp();
        sqlx::query(
            "INSERT INTO room (id, workspace_id, name, capacity, created_at, updated_at, hlc, origin_device)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET name = excluded.name, capacity = excluded.capacity,
                updated_at = excluded.updated_at, hlc = excluded.hlc, origin_device = excluded.origin_device",
        )
        .bind(&id)
        .bind(&ws)
        .bind(name)
        .bind(capacity)
        .bind(s.now)
        .bind(s.now)
        .bind(&s.hlc)
        .bind(&s.device)
        .execute(tx.conn())
        .await?;
        let room = Room {
            id: id.clone(),
            name: name.to_string(),
            capacity,
        };
        tx.log_upsert("room", &id, &s, &room).await?;
        tx.commit().await?;
        Ok(room)
    }

    pub async fn delete_room(&self, id: &str) -> Result<()> {
        let mut tx = self.begin().await?;
        super::soft_delete(&mut tx, "room", id).await?;
        tx.commit().await
    }
}
