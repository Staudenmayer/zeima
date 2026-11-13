//! Canonical ids for sync.
//!
//! Most rows are created once, on one device, with a random id, and that id
//! identifies them everywhere. Some rows are created by *every* device:
//! work settings and the country selection exist once per user, the built-in
//! absence types exist once per key, and two devices that set a schedule for
//! the same start date mean the same schedule. Left alone they would become
//! duplicates (or unique-index conflicts) on every device that joins.
//!
//! Sync therefore refers to such rows by a *canonical id*: a UUID derived from
//! what identifies the row (`rule`). Local ids do not change; `sync_alias`
//! maps between the two, and references to such rows (an absence's type) are
//! translated on the way out and in.

use serde_json::{Map, Value};
use sqlx::SqliteConnection;
use uuid::Uuid;

use crate::Result;

/// Namespace of the derived ids (fixed; changing it would split every account).
const NAMESPACE: Uuid = Uuid::from_bytes([
    0x5a, 0x65, 0x69, 0x6d, 0x61, 0x2d, 0x73, 0x79, 0x6e, 0x63, 0x2d, 0x69, 0x64, 0x73, 0x00, 0x01,
]);

/// How a table's rows are identified across devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// The row's own id (created on one device, shared by everyone).
    Plain,
    /// One row per user.
    Singleton,
    /// Rows with the same value in this payload field are the same row.
    /// A row without a value (a custom absence type has no key) is `Plain`.
    ByField(&'static str),
}

pub fn rule(table: &str) -> Rule {
    match table {
        "work_settings" | "jurisdiction_selection" => Rule::Singleton,
        "work_schedule" => Rule::ByField("validFrom"),
        "absence_type" => Rule::ByField("key"),
        _ => Rule::Plain,
    }
}

/// Fields that hold the id of a row of another table: (table, field, target table).
pub const REFERENCES: &[(&str, &str, &str)] = &[("absence", "typeId", "absence_type")];

/// The canonical id of a row, from its payload fields when it has any.
pub fn canonical_for(table: &str, local_id: &str, fields: Option<&Map<String, Value>>) -> String {
    let name = match rule(table) {
        Rule::Plain => return local_id.to_string(),
        Rule::Singleton => table.to_string(),
        Rule::ByField(field) => match fields.and_then(|f| f.get(field)).and_then(Value::as_str) {
            Some(value) if !value.is_empty() => format!("{table}:{field}={value}"),
            _ => return local_id.to_string(),
        },
    };
    Uuid::new_v5(&NAMESPACE, name.as_bytes()).to_string()
}

pub async fn canonical_of_local(
    conn: &mut SqliteConnection,
    table: &str,
    local_id: &str,
) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT canonical_id FROM sync_alias WHERE table_name = ? AND local_id = ?")
            .bind(table)
            .bind(local_id)
            .fetch_optional(&mut *conn)
            .await?,
    )
}

pub async fn local_of_canonical(
    conn: &mut SqliteConnection,
    table: &str,
    canonical_id: &str,
) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT local_id FROM sync_alias WHERE table_name = ? AND canonical_id = ?")
            .bind(table)
            .bind(canonical_id)
            .fetch_optional(&mut *conn)
            .await?,
    )
}

/// Remembers that `local_id` stands for `canonical_id`. Replaces an older
/// pairing of either side.
pub async fn remember(
    conn: &mut SqliteConnection,
    table: &str,
    local_id: &str,
    canonical_id: &str,
) -> Result<()> {
    if local_id == canonical_id {
        return Ok(());
    }
    sqlx::query("DELETE FROM sync_alias WHERE table_name = ? AND (local_id = ? OR canonical_id = ?)")
        .bind(table)
        .bind(local_id)
        .bind(canonical_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("INSERT INTO sync_alias (table_name, local_id, canonical_id) VALUES (?, ?, ?)")
        .bind(table)
        .bind(local_id)
        .bind(canonical_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// A reference as the server knows it: the local id of the target row, as its canonical id.
pub async fn reference_out(
    conn: &mut SqliteConnection,
    target_table: &str,
    local_id: &str,
) -> Result<String> {
    Ok(canonical_of_local(conn, target_table, local_id)
        .await?
        .unwrap_or_else(|| local_id.to_string()))
}

/// A reference from the server: the canonical id of the target row, as its local id.
pub async fn reference_in(
    conn: &mut SqliteConnection,
    target_table: &str,
    canonical_id: &str,
) -> Result<String> {
    Ok(local_of_canonical(conn, target_table, canonical_id)
        .await?
        .unwrap_or_else(|| canonical_id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fields(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn plain_rows_keep_their_id() {
        assert_eq!(canonical_for("project", "abc", None), "abc");
        assert_eq!(
            canonical_for("absence", "abc", Some(&fields(json!({"typeId": "x"})))),
            "abc"
        );
    }

    #[test]
    fn singletons_have_one_id_everywhere() {
        let a = canonical_for("work_settings", "local-a", None);
        let b = canonical_for(
            "work_settings",
            "local-b",
            Some(&fields(json!({"deductBreaks": true}))),
        );
        assert_eq!(a, b);
        assert_ne!(a, canonical_for("jurisdiction_selection", "local-a", None));
        assert!(Uuid::parse_str(&a).is_ok());
    }

    #[test]
    fn keyed_rows_share_an_id_when_their_key_matches() {
        let vacation_a = canonical_for("absence_type", "a", Some(&fields(json!({"key": "vacation"}))));
        let vacation_b = canonical_for("absence_type", "b", Some(&fields(json!({"key": "vacation"}))));
        let sick = canonical_for("absence_type", "c", Some(&fields(json!({"key": "sick"}))));
        assert_eq!(vacation_a, vacation_b);
        assert_ne!(vacation_a, sick);
        // A custom type has no key and stays itself.
        assert_eq!(
            canonical_for("absence_type", "own", Some(&fields(json!({"key": null})))),
            "own"
        );
        assert_eq!(canonical_for("absence_type", "own", None), "own");
    }

    #[test]
    fn schedules_are_identified_by_their_start_date() {
        let a = canonical_for(
            "work_schedule",
            "a",
            Some(&fields(json!({"validFrom": "2026-01-01"}))),
        );
        let b = canonical_for(
            "work_schedule",
            "b",
            Some(&fields(json!({"validFrom": "2026-01-01"}))),
        );
        let c = canonical_for(
            "work_schedule",
            "c",
            Some(&fields(json!({"validFrom": "2026-06-01"}))),
        );
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn the_derived_ids_never_change() {
        // These are stored on servers; a change would split every account.
        assert_eq!(
            canonical_for("work_settings", "x", None),
            Uuid::new_v5(&NAMESPACE, b"work_settings").to_string()
        );
        assert_eq!(NAMESPACE.to_string(), "5a65696d-612d-7379-6e63-2d6964730001");
    }
}
