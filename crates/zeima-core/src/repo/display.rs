//! The part of the app settings that follows the user between devices
//! (language, formats, time zone, ...). The app decides which settings those
//! are; here they are one JSON object that is stored, change-logged and synced
//! like the work settings.

use serde_json::Value;

use crate::{Core, Error, Result};

impl Core {
    /// The synced display settings, if any device has saved them yet.
    pub async fn display_settings(&self) -> Result<Option<Value>> {
        let json: Option<String> = sqlx::query_scalar(
            "SELECT settings_json FROM display_settings WHERE user_id = ? AND deleted_at IS NULL",
        )
        .bind(&self.ctx().user_id)
        .fetch_optional(self.pool())
        .await?;
        json.map(|j| serde_json::from_str(&j))
            .transpose()
            .map_err(Error::from)
    }

    /// Stores new display settings and queues them for sync. Does nothing
    /// when they equal the stored ones, so applying what sync delivered does
    /// not send it back.
    pub async fn set_display_settings(&self, settings: &Value) -> Result<()> {
        if !settings.is_object() {
            return Err(Error::validation("settings", "must be an object"));
        }
        if self.display_settings().await?.as_ref() == Some(settings) {
            return Ok(());
        }
        let mut tx = self.begin().await?;
        let user = tx.ctx().user_id.clone();
        let existing: Option<String> =
            sqlx::query_scalar("SELECT id FROM display_settings WHERE user_id = ? AND deleted_at IS NULL")
                .bind(&user)
                .fetch_optional(tx.conn())
                .await?;
        let id = existing.unwrap_or_else(crate::ids::new_id);
        let s = tx.stamp();
        sqlx::query(
            "INSERT INTO display_settings (id, workspace_id, user_id, settings_json, created_at, updated_at,
                hlc, origin_device)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET settings_json = excluded.settings_json,
                updated_at = excluded.updated_at, hlc = excluded.hlc, origin_device = excluded.origin_device",
        )
        .bind(&id)
        .bind(tx.ctx().workspace_id.clone())
        .bind(&user)
        .bind(serde_json::to_string(settings)?)
        .bind(s.now)
        .bind(s.now)
        .bind(&s.hlc)
        .bind(&s.device)
        .execute(tx.conn())
        .await?;
        tx.log_upsert("display_settings", &id, &s, settings).await?;
        tx.commit().await
    }
}
