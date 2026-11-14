//! The sync outbox: turns the change log into the field-level changes the
//! server merges (SPEC §6). The change log holds whole rows; the server needs
//! only the fields that changed, or an edit made on one device would carry the
//! stale values of every other field and overwrite another device's edit. So
//! each logged row is compared with `sync_state`, the last known server
//! version, and only the differences are sent.

use std::collections::BTreeMap;

use serde_json::Value;
use sqlx::Row;

use super::sync_groups::group_payload;
use super::sync_ids::{canonical_for, canonical_of_local, reference_out, remember, REFERENCES};
use crate::sync::{merge, Actor, Change, Op, Outcome, RowState};
use crate::{Core, Result};

/// Tables the server accepts (`server/src/modules/sync/routes.ts`). Changes
/// to other tables stay in the log until the server learns them.
pub const SYNCED_TABLES: [&str; 18] = [
    "project",
    "tag",
    "time_entry",
    "pause",
    "activity",
    "cost_center",
    "cost_center_assignment",
    "jurisdiction_selection",
    "work_settings",
    "work_schedule",
    "absence_type",
    "absence",
    "custom_holiday",
    "balance_adjustment",
    "display_settings",
    "timesheet_submission",
    "work_location",
    "org_policy",
];

/// The synced tables as an SQL list. sqlx wants literal SQL, so the list is a
/// macro; a test keeps it equal to `SYNCED_TABLES`.
macro_rules! synced_tables_sql {
    () => {
        "'project', 'tag', 'time_entry', 'pause', 'activity', 'cost_center', 'cost_center_assignment', \
         'jurisdiction_selection', 'work_settings', 'work_schedule', 'absence_type', 'absence', \
         'custom_holiday', 'balance_adjustment', 'display_settings', 'timesheet_submission', 'work_location', 'org_policy'"
    };
}

/// Fields that name this device's own workspace and user. Every device has
/// its own ids for them, so they are not synced; pulled rows get the local
/// values instead.
pub const LOCAL_FIELDS: [&str; 3] = ["id", "workspaceId", "userId"];

/// Upper bound for one push.
pub const BATCH: i64 = 500;

/// A batch of changes to push.
#[derive(Debug, Default)]
pub struct Outbox {
    pub changes: Vec<Change>,
    /// Log rows covered by the batch (including ones that needed no change).
    seqs: Vec<i64>,
}

impl Outbox {
    pub fn is_empty(&self) -> bool {
        self.seqs.is_empty()
    }
}

fn parse_state(text: &str) -> Result<RowState> {
    Ok(serde_json::from_str(text)?)
}

async fn load_state(
    conn: &mut sqlx::SqliteConnection,
    table: &str,
    row_id: &str,
) -> Result<Option<RowState>> {
    let text: Option<String> =
        sqlx::query_scalar("SELECT state FROM sync_state WHERE table_name = ? AND row_id = ?")
            .bind(table)
            .bind(row_id)
            .fetch_optional(&mut *conn)
            .await?;
    text.as_deref().map(parse_state).transpose()
}

/// The fields of `payload` that differ from what the server has.
/// Whether a field names this device's own workspace or user (see
/// `LOCAL_FIELDS`). In a shared workspace the user is the account and is sent.
pub fn is_local_field(name: &str, shared: bool) -> bool {
    LOCAL_FIELDS.contains(&name) && !(shared && name == "userId")
}

fn changed_fields(payload: &Value, state: Option<&RowState>, shared: bool) -> BTreeMap<String, Value> {
    let Some(object) = payload.as_object() else {
        return BTreeMap::new();
    };
    object
        .iter()
        .filter(|(name, _)| !is_local_field(name, shared))
        .filter(|(name, value)| {
            state
                .and_then(|s| s.fields.get(*name))
                .is_none_or(|known| known.v != **value)
        })
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

impl Core {
    /// The next batch of unpushed changes, oldest first. Call
    /// [`Core::sync_mark_pushed`] with the server's answers.
    pub async fn sync_outbox(&self, limit: i64) -> Result<Outbox> {
        let rows = sqlx::query(concat!(
            "SELECT seq, table_name, row_id, op, hlc, payload FROM change_log \
             WHERE pushed_at IS NULL AND workspace_id IS ? AND table_name IN (",
            synced_tables_sql!(),
            ") ORDER BY seq LIMIT ?"
        ))
        .bind(&self.ctx().workspace_id)
        .bind(limit)
        .fetch_all(self.pool())
        .await?;

        let mut conn = self.pool().acquire().await?;
        // Several log rows can touch one row; later ones diff against the earlier.
        let mut working: BTreeMap<(String, String), Option<RowState>> = BTreeMap::new();
        let mut outbox = Outbox::default();
        for row in rows {
            let seq: i64 = row.get("seq");
            let table: String = row.get("table_name");
            let row_id: String = row.get("row_id");
            let op: String = row.get("op");
            let hlc: String = row.get("hlc");
            let payload: Option<String> = row.get("payload");
            outbox.seqs.push(seq);

            // The server knows rows that every device creates for itself by a
            // canonical id (sync_ids.rs); other rows by their own.
            let mut payload: Value = match (op.as_str(), payload.as_deref()) {
                ("delete", _) | (_, None) => Value::Null,
                (_, Some(text)) => serde_json::from_str(text)?,
            };
            let row_id = if op == "delete" {
                canonical_of_local(&mut conn, &table, &row_id)
                    .await?
                    .unwrap_or(row_id)
            } else {
                let canonical = canonical_for(&table, &row_id, payload.as_object());
                remember(&mut conn, &table, &row_id, &canonical).await?;
                canonical
            };
            for (owner, field, target) in REFERENCES {
                if *owner != table {
                    continue;
                }
                if let Some(Value::String(local)) = payload.get(*field).cloned() {
                    let canonical = reference_out(&mut conn, target, &local).await?;
                    payload[*field] = Value::String(canonical);
                }
            }

            // Fields that must change together travel as one (sync_groups.rs).
            group_payload(&table, &mut payload);

            let key = (table.clone(), row_id.clone());
            if !working.contains_key(&key) {
                let known = load_state(&mut conn, &table, &row_id).await?;
                working.insert(key.clone(), known);
            }
            let known = working.get_mut(&key).expect("just inserted");

            let change = if op == "delete" {
                Change {
                    table,
                    row_id,
                    op: Op::Delete,
                    hlc,
                    fields: BTreeMap::new(),
                }
            } else {
                let fields = changed_fields(&payload, known.as_ref(), self.ctx().shared);
                if fields.is_empty() {
                    continue;
                }
                Change {
                    table,
                    row_id,
                    op: Op::Upsert,
                    hlc,
                    fields,
                }
            };
            // What the server will hold once it takes the change.
            let (next, _) = merge(known.as_ref(), &change, Actor { manager: true });
            *known = Some(next);
            outbox.changes.push(change);
        }
        Ok(outbox)
    }

    /// Changes waiting to be sent. Unlike `pending_changes` this leaves out
    /// the tables that never sync (the user and workspace records), which
    /// would otherwise stay in the count forever.
    pub async fn sync_pending(&self) -> Result<i64> {
        Ok(sqlx::query_scalar(concat!(
            "SELECT count(*) FROM change_log WHERE pushed_at IS NULL AND table_name IN (",
            synced_tables_sql!(),
            ")"
        ))
        .fetch_one(self.pool())
        .await?)
    }

    /// Records the server's answer: marks the batch pushed and remembers the
    /// new server state of each row. `results` has one entry per change in
    /// the batch; a rejected change is not remembered (the server did not
    /// take it) but is not retried either: it would be refused again.
    pub async fn sync_mark_pushed(&self, outbox: &Outbox, results: &[Outcome]) -> Result<()> {
        if results.len() != outbox.changes.len() {
            return Err(crate::Error::validation(
                "results",
                "the server must answer every change",
            ));
        }
        let mut tx = self.pool().begin().await?;
        for (change, outcome) in outbox.changes.iter().zip(results) {
            if matches!(outcome, Outcome::Rejected { .. }) {
                continue;
            }
            let known = load_state(&mut tx, &change.table, &change.row_id).await?;
            let (next, _) = merge(known.as_ref(), change, Actor { manager: true });
            sqlx::query(
                "INSERT INTO sync_state (table_name, row_id, state) VALUES (?, ?, ?) \
                 ON CONFLICT (table_name, row_id) DO UPDATE SET state = excluded.state",
            )
            .bind(&change.table)
            .bind(&change.row_id)
            .bind(serde_json::to_string(&next)?)
            .execute(&mut *tx)
            .await?;
        }
        let now = self.now_ms();
        for seq in &outbox.seqs {
            sqlx::query("UPDATE change_log SET pushed_at = ? WHERE seq = ?")
                .bind(now)
                .bind(seq)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sql_list_matches_the_synced_tables() {
        let listed: Vec<&str> = synced_tables_sql!()
            .split(',')
            .map(|t| t.trim().trim_matches('\''))
            .collect();
        assert_eq!(listed, SYNCED_TABLES);
    }
}
