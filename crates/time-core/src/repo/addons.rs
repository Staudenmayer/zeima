//! Installed addons and their key-value storage (device-local, not synced).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use super::not_found;
use crate::addon::Permission;
use crate::db::Core;
use crate::{Error, Result};

/// Limits for an addon's key-value storage.
pub const KV_MAX_KEYS: i64 = 10_000;
pub const KV_MAX_VALUE_BYTES: usize = 1024 * 1024;
pub const KV_MAX_KEY_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct AddonRecord {
    pub id: String,
    pub version: String,
    pub enabled: bool,
    pub granted: Vec<Permission>,
    /// Settings form values by key.
    pub settings: BTreeMap<String, String>,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub installed_at: i64,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub updated_at: i64,
}

fn from_row(r: &SqliteRow) -> Result<AddonRecord> {
    Ok(AddonRecord {
        id: r.try_get("id")?,
        version: r.try_get("version")?,
        enabled: r.try_get("enabled")?,
        granted: serde_json::from_str(&r.try_get::<String, _>("granted_json")?)?,
        settings: serde_json::from_str(&r.try_get::<String, _>("settings_json")?)?,
        installed_at: r.try_get("installed_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

impl Core {
    pub async fn list_addons(&self) -> Result<Vec<AddonRecord>> {
        let rows = sqlx::query("SELECT * FROM addon ORDER BY id")
            .fetch_all(self.pool())
            .await?;
        rows.iter().map(from_row).collect()
    }

    pub async fn get_addon(&self, id: &str) -> Result<AddonRecord> {
        let row = sqlx::query("SELECT * FROM addon WHERE id = ?")
            .bind(id)
            .fetch_optional(self.pool())
            .await?
            .ok_or_else(|| not_found("addon", id))?;
        from_row(&row)
    }

    /// Records an installed (or updated) addon. An update keeps the enabled
    /// flag and settings; the granted permissions are replaced.
    pub async fn save_addon(&self, id: &str, version: &str, granted: &[Permission]) -> Result<AddonRecord> {
        let now = self.now_ms();
        sqlx::query(
            "INSERT INTO addon (id, version, granted_json, installed_at, updated_at) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET version = excluded.version,
                granted_json = excluded.granted_json, updated_at = excluded.updated_at",
        )
        .bind(id)
        .bind(version)
        .bind(serde_json::to_string(granted)?)
        .bind(now)
        .bind(now)
        .execute(self.pool())
        .await?;
        self.get_addon(id).await
    }

    pub async fn set_addon_enabled(&self, id: &str, enabled: bool) -> Result<AddonRecord> {
        let done = sqlx::query("UPDATE addon SET enabled = ?, updated_at = ? WHERE id = ?")
            .bind(enabled)
            .bind(self.now_ms())
            .bind(id)
            .execute(self.pool())
            .await?;
        if done.rows_affected() == 0 {
            return Err(not_found("addon", id));
        }
        self.get_addon(id).await
    }

    pub async fn set_addon_settings(
        &self,
        id: &str,
        settings: &BTreeMap<String, String>,
    ) -> Result<AddonRecord> {
        let done = sqlx::query("UPDATE addon SET settings_json = ?, updated_at = ? WHERE id = ?")
            .bind(serde_json::to_string(settings)?)
            .bind(self.now_ms())
            .bind(id)
            .execute(self.pool())
            .await?;
        if done.rows_affected() == 0 {
            return Err(not_found("addon", id));
        }
        self.get_addon(id).await
    }

    /// Removes the addon and its storage.
    pub async fn delete_addon(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM addon WHERE id = ?")
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    pub async fn addon_kv_get(&self, addon_id: &str, key: &str) -> Result<Option<String>> {
        Ok(
            sqlx::query_scalar("SELECT value FROM addon_kv WHERE addon_id = ? AND key = ?")
                .bind(addon_id)
                .bind(key)
                .fetch_optional(self.pool())
                .await?,
        )
    }

    /// Stores a value; `None` deletes the key.
    pub async fn addon_kv_set(&self, addon_id: &str, key: &str, value: Option<&str>) -> Result<()> {
        if key.is_empty() || key.len() > KV_MAX_KEY_BYTES {
            return Err(Error::validation("key", format!("1–{KV_MAX_KEY_BYTES} bytes")));
        }
        let Some(value) = value else {
            sqlx::query("DELETE FROM addon_kv WHERE addon_id = ? AND key = ?")
                .bind(addon_id)
                .bind(key)
                .execute(self.pool())
                .await?;
            return Ok(());
        };
        if value.len() > KV_MAX_VALUE_BYTES {
            return Err(Error::validation("value", "at most 1 MB"));
        }
        let mut tx = self.pool().begin().await?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM addon_kv WHERE addon_id = ? AND key != ?")
            .bind(addon_id)
            .bind(key)
            .fetch_one(&mut *tx)
            .await?;
        if count >= KV_MAX_KEYS {
            return Err(Error::validation(
                "key",
                format!("at most {KV_MAX_KEYS} keys per addon"),
            ));
        }
        sqlx::query(
            "INSERT INTO addon_kv (addon_id, key, value) VALUES (?, ?, ?)
             ON CONFLICT (addon_id, key) DO UPDATE SET value = excluded.value",
        )
        .bind(addon_id)
        .bind(key)
        .bind(value)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Keys starting with `prefix`, sorted.
    pub async fn addon_kv_keys(&self, addon_id: &str, prefix: &str) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT key FROM addon_kv WHERE addon_id = ? AND substr(key, 1, length(?)) = ? ORDER BY key",
        )
        .bind(addon_id)
        .bind(prefix)
        .bind(prefix)
        .fetch_all(self.pool())
        .await?)
    }
}
