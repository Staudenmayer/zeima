//! Whole-database operations: full JSON export and file backups.

use std::path::Path;

use serde_json::{Map, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Column, Row, TypeInfo, ValueRef};

use crate::db::Core;
use crate::Result;

/// Tables in the full export. The names are interpolated into SQL, so they
/// must come from this fixed list. `true` = has a `deleted_at` tombstone.
const EXPORT_TABLES: &[(&str, bool)] = &[
    ("workspace", true),
    ("app_user", true),
    ("activity", true),
    ("project", true),
    ("tag", true),
    ("cost_center", true),
    ("cost_center_assignment", true),
    ("time_entry", true),
    ("time_entry_tag", false),
    ("pause", true),
    ("jurisdiction_selection", true),
    ("work_schedule", true),
    ("work_settings", true),
    ("absence_type", true),
    ("absence", true),
    ("custom_holiday", true),
    ("balance_adjustment", true),
];

/// Bumped when the export layout changes incompatibly.
const EXPORT_FORMAT: u32 = 1;

fn row_to_json(row: &SqliteRow) -> Result<Value> {
    let mut obj = Map::new();
    for (i, col) in row.columns().iter().enumerate() {
        let raw = row.try_get_raw(i)?;
        let value = if raw.is_null() {
            Value::Null
        } else {
            match raw.type_info().name() {
                "INTEGER" => Value::from(row.try_get::<i64, _>(i)?),
                "REAL" => Value::from(row.try_get::<f64, _>(i)?),
                "BLOB" => Value::Null,
                _ => {
                    let text: String = row.try_get(i)?;
                    // JSON columns are embedded as JSON, not as strings.
                    if col.name().ends_with("_json") {
                        serde_json::from_str(&text).unwrap_or(Value::String(text))
                    } else {
                        Value::String(text)
                    }
                }
            }
        };
        obj.insert(col.name().to_string(), value);
    }
    Ok(Value::Object(obj))
}

impl Core {
    /// All live rows of all user data tables, as
    /// `{ format, exportedAt, tables: { name: [row, …] } }`.
    pub async fn export_json(&self) -> Result<Value> {
        let mut tables = Map::new();
        for &(table, soft_delete) in EXPORT_TABLES {
            let filter = if soft_delete {
                " WHERE deleted_at IS NULL"
            } else {
                ""
            };
            let sql = format!("SELECT * FROM {table}{filter}");
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
                .fetch_all(self.pool())
                .await?;
            let rows = rows.iter().map(row_to_json).collect::<Result<Vec<_>>>()?;
            tables.insert(table.to_string(), Value::Array(rows));
        }
        Ok(serde_json::json!({
            "format": EXPORT_FORMAT,
            "exportedAt": self.now_ms(),
            "tables": tables,
        }))
    }

    /// Writes a consistent copy of the database to `path` (must not exist).
    pub async fn backup_to(&self, path: &Path) -> Result<()> {
        sqlx::query("VACUUM INTO ?")
            .bind(path.to_string_lossy().into_owned())
            .execute(self.pool())
            .await?;
        Ok(())
    }

    /// `PRAGMA quick_check`; `None` when the database is fine.
    pub async fn integrity_problem(&self) -> Result<Option<String>> {
        let result: String = sqlx::query_scalar("PRAGMA quick_check")
            .fetch_one(self.pool())
            .await?;
        Ok((result != "ok").then_some(result))
    }
}
