//! Applying the server's changes to the local database (SPEC §6.3).
//!
//! The server sends the merged state of each changed row: per field the value
//! and the HLC that set it. For every row this merges that state into
//! `sync_state` (the last known server version) and then writes the fields
//! that are new to the typed SQLite row. Writes here do not go through the
//! change log: they came from the server and must not be pushed back.
//!
//! Three details keep devices from fighting:
//!  - `workspaceId` and `userId` are local (see `LOCAL_FIELDS`); pulled rows
//!    get this device's values.
//!  - A field is not overwritten while this device has an unpushed change to
//!    the row that is newer than the incoming write; the local edit wins,
//!    and is pushed.
//!  - A row that cannot be written (a unique alias taken locally, say) is
//!    recorded in `sync_conflict` and the rest go on.
//!
//! Foreign keys are off while applying: a page of changes can arrive child
//! before parent, and the parent comes with a later page.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::Value;
use sqlx::{Connection, Row, SqliteConnection};

use super::sync_groups::{expand, group_payload};
use super::sync_ids::{local_of_canonical, reference_in, remember, rule, Rule, REFERENCES};
use super::sync_outbox::{is_local_field, SYNCED_TABLES};
use crate::hlc;
use crate::sync::{merge_state, RowState, Versioned};
use crate::{Core, Result};

/// One row of `GET /v1/sync/pull`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRow {
    pub workspace_id: String,
    pub table: String,
    pub row_id: String,
    pub fields: BTreeMap<String, Versioned>,
    pub deleted: Option<String>,
}

/// One field of a conflicting row, as the account has it and as this device has it.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldView {
    pub field: String,
    /// The account's value.
    pub server: Value,
    /// This device's value; `None` when the row does not exist here.
    pub local: Option<Value>,
}

/// A server change that could not be written locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncConflict {
    pub table: String,
    pub row_id: String,
    /// The server's name for the row, if it has one.
    pub name: Option<String>,
    pub message: String,
    pub at: i64,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ApplyReport {
    /// Rows written to the typed tables.
    pub applied: usize,
    /// Rows that could not be written (see `sync_conflict`).
    pub conflicts: usize,
}

/// `budgetWarnPct` → `budget_warn_pct`.
fn snake(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            out.push('_');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

enum Bind {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
}

/// A JSON scalar as an SQLite value; arrays and objects are not columns.
fn bind_of(value: &Value) -> Option<Bind> {
    Some(match value {
        Value::Null => Bind::Null,
        Value::Bool(b) => Bind::Int(i64::from(*b)),
        Value::Number(n) => match n.as_i64() {
            Some(i) => Bind::Int(i),
            None => Bind::Real(n.as_f64()?),
        },
        Value::String(s) => Bind::Text(s.clone()),
        Value::Array(_) | Value::Object(_) => return None,
    })
}

/// The columns of a table and which of them cannot be NULL.
struct Cols(BTreeMap<String, bool>);

impl Cols {
    fn contains(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    fn not_null(&self, name: &str) -> bool {
        self.0.get(name).copied().unwrap_or(false)
    }
}

async fn columns(conn: &mut SqliteConnection, table: &str) -> std::result::Result<Cols, sqlx::Error> {
    // `table` is one of SYNCED_TABLES.
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!("PRAGMA table_info({table})")))
        .fetch_all(&mut *conn)
        .await?;
    Ok(Cols(
        rows.iter()
            .map(|r| (r.get::<String, _>("name"), r.get::<i64, _>("notnull") != 0))
            .collect(),
    ))
}

/// A database error from applying a change, in words the user can act on.
/// Anything not recognized is shown as it is.
fn friendly_conflict(message: &str) -> String {
    // SQLite names a partial unique index by its columns, and an expression
    // index by its name, so each case lists what it may call itself.
    let known: &[(&[&str], &str)] = &[
        (
            &["time_entry_one_running", "time_entry.user_id"],
            "A timer is already running on this device. Stop it, then try again.",
        ),
        (
            &["pause_one_open", "pause.user_id"],
            "A pause is already open on this device. End it, then try again.",
        ),
        (
            &["project_alias_unique", "project.workspace_id"],
            "Another project on this device already uses this alias. Change one of them, then try again.",
        ),
        (
            &["tag_name_unique", "tag.workspace_id"],
            "A tag with this name already exists on this device. Rename one of them, then try again.",
        ),
        (
            &["work_schedule_valid_from", "work_schedule.user_id"],
            "This device already has a schedule starting on that day.",
        ),
        (
            &["CHECK constraint failed"],
            "The change does not fit this device's data (for example an end before the start).",
        ),
    ];
    known
        .iter()
        .find(|(needles, _)| needles.iter().any(|n| message.contains(n)))
        .map_or_else(|| message.to_string(), |(_, text)| (*text).to_string())
}

/// A typed row as JSON with the sync's field names, or `None` if it is missing.
async fn local_row_fields(
    conn: &mut SqliteConnection,
    table: &str,
    id: &str,
) -> Result<Option<BTreeMap<String, Value>>> {
    let cols = columns(conn, table).await?;
    let Some(row) = sqlx::query(sqlx::AssertSqlSafe(format!("SELECT * FROM {table} WHERE id = ?")))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
    else {
        return Ok(None);
    };
    let mut out = BTreeMap::new();
    for name in cols.0.keys() {
        let value = if let Ok(Some(i)) = row.try_get::<Option<i64>, _>(name.as_str()) {
            Value::from(i)
        } else if let Ok(Some(f)) = row.try_get::<Option<f64>, _>(name.as_str()) {
            Value::from(f)
        } else if let Ok(Some(t)) = row.try_get::<Option<String>, _>(name.as_str()) {
            Value::String(t)
        } else {
            Value::Null
        };
        out.insert(camel(name), value);
    }
    // JSON columns hold several fields.
    for (column, field) in [
        ("daysJson", "days"),
        ("memberIdsJson", "memberIds"),
        ("attendeeIdsJson", "attendeeIds"),
    ] {
        if let Some(Value::String(text)) = out.remove(column) {
            out.insert(
                field.to_string(),
                serde_json::from_str(&text).unwrap_or(Value::String(text)),
            );
        }
    }
    if let Some(Value::String(text)) = out.remove("settingsJson") {
        if let Ok(Value::Object(map)) = serde_json::from_str(&text) {
            out.extend(map);
        }
    }
    Ok(Some(out))
}

/// `budget_warn_pct` → `budgetWarnPct`.
fn camel(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper = false;
    for c in name.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

async fn load_state(conn: &mut SqliteConnection, table: &str, row_id: &str) -> Result<RowState> {
    let text: Option<String> =
        sqlx::query_scalar("SELECT state FROM sync_state WHERE table_name = ? AND row_id = ?")
            .bind(table)
            .bind(row_id)
            .fetch_optional(&mut *conn)
            .await?;
    Ok(match text {
        Some(t) => serde_json::from_str(&t)?,
        None => RowState::default(),
    })
}

/// What `write_typed` did.
#[derive(Default)]
struct Written {
    changed: bool,
    /// The incoming values that were written, by field name.
    values: Vec<(String, Value)>,
}

/// This device's unpushed changes to one row.
struct Pending {
    table: String,
    row_id: String,
    /// Newest unpushed change of any kind.
    newest: Option<String>,
    /// Newest unpushed delete.
    delete: Option<String>,
    /// Fields whose value here differs from what the server last had, with
    /// the HLC of the local edit that gave them that value.
    changed: BTreeMap<String, String>,
}

impl Pending {
    /// Whether an unpushed local edit of `field` is newer than the incoming write.
    fn protects(&self, field: &str, incoming: &str) -> bool {
        self.changed
            .get(field)
            .is_some_and(|edited| edited.as_str() >= incoming)
    }

    /// The server's value replaced the local one in the row; the unpushed
    /// whole-row payloads must say so too, or the next push would send the old
    /// value back as if it were a new edit.
    async fn follow(&self, conn: &mut SqliteConnection, values: &[(String, Value)]) -> Result<()> {
        if values.is_empty() || self.newest.is_none() {
            return Ok(());
        }
        let rows = sqlx::query(
            "SELECT seq, payload FROM change_log \
             WHERE pushed_at IS NULL AND table_name = ? AND row_id = ? AND op = 'upsert'",
        )
        .bind(&self.table)
        .bind(&self.row_id)
        .fetch_all(&mut *conn)
        .await?;
        for row in rows {
            let seq: i64 = row.get("seq");
            let Some(text): Option<String> = row.get("payload") else {
                continue;
            };
            let mut payload: Value = serde_json::from_str(&text)?;
            if let Some(object) = payload.as_object_mut() {
                for (name, value) in values {
                    object.insert(name.clone(), value.clone());
                }
            }
            sqlx::query("UPDATE change_log SET payload = ? WHERE seq = ?")
                .bind(serde_json::to_string(&payload)?)
                .bind(seq)
                .execute(&mut *conn)
                .await?;
        }
        Ok(())
    }
}

async fn load_pending(
    conn: &mut SqliteConnection,
    table: &str,
    row_id: &str,
    known: &RowState,
    shared: bool,
) -> Result<Pending> {
    let rows = sqlx::query(
        "SELECT op, hlc, payload FROM change_log \
         WHERE pushed_at IS NULL AND table_name = ? AND row_id = ? ORDER BY seq",
    )
    .bind(table)
    .bind(row_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut pending = Pending {
        table: table.to_string(),
        row_id: row_id.to_string(),
        newest: None,
        delete: None,
        changed: BTreeMap::new(),
    };
    // Walk the unpushed whole-row payloads in order. A field was edited at
    // the HLC of the first row that gave it its current value.
    let mut current: BTreeMap<String, Value> = known
        .fields
        .iter()
        .map(|(k, f)| (k.clone(), f.v.clone()))
        .collect();
    let mut edited: BTreeMap<String, String> = BTreeMap::new();
    for row in rows {
        let op: String = row.get("op");
        let hlc: String = row.get("hlc");
        if pending.newest.as_ref().is_none_or(|n| hlc > *n) {
            pending.newest = Some(hlc.clone());
        }
        if op == "delete" {
            if pending.delete.as_ref().is_none_or(|d| hlc > *d) {
                pending.delete = Some(hlc);
            }
        } else if let Some(text) = row.get::<Option<String>, _>("payload") {
            let mut payload: Value = serde_json::from_str(&text)?;
            group_payload(table, &mut payload);
            for (name, value) in payload.as_object().into_iter().flatten() {
                if is_local_field(name, shared) {
                    continue;
                }
                if current.get(name) != Some(value) {
                    current.insert(name.clone(), value.clone());
                    edited.insert(name.clone(), hlc.clone());
                }
            }
        }
    }
    // Only fields that still differ from the server's version count.
    for (name, hlc) in edited {
        if known
            .fields
            .get(&name)
            .is_none_or(|k| Some(&k.v) != current.get(&name))
        {
            pending.changed.insert(name, hlc);
        }
    }
    Ok(pending)
}

fn newest(fields: &BTreeMap<String, &Versioned>) -> Option<String> {
    fields.values().map(|f| f.h.clone()).max()
}

impl Core {
    /// Applies pulled rows. Rows of other workspaces and of tables that do
    /// not sync are skipped.
    pub async fn sync_apply(&self, rows: &[RemoteRow]) -> Result<ApplyReport> {
        let mut conn = self.pool().acquire().await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;
        let result = self.apply_rows(&mut conn, rows, false).await;
        let _ = sqlx::query("PRAGMA foreign_keys = ON").execute(&mut *conn).await;
        let (report, newest_hlc) = result?;
        if let Some(h) = newest_hlc {
            self.observe_hlc(&h);
        }
        Ok(report)
    }

    async fn apply_rows(
        &self,
        conn: &mut SqliteConnection,
        rows: &[RemoteRow],
        force: bool,
    ) -> Result<(ApplyReport, Option<String>)> {
        let mut tx = conn.begin().await?;
        let mut report = ApplyReport::default();
        let mut newest_hlc: Option<String> = None;
        let mut days_changed = false;

        for row in rows {
            if !SYNCED_TABLES.contains(&row.table.as_str()) {
                continue;
            }
            let remote = RowState {
                fields: row.fields.clone(),
                deleted: row.deleted.clone(),
            };
            for h in remote.fields.values().map(|f| &f.h).chain(remote.deleted.iter()) {
                if newest_hlc.as_ref().is_none_or(|n| h > n) {
                    newest_hlc = Some(h.clone());
                }
            }

            let known = load_state(&mut tx, &row.table, &row.row_id).await?;
            let merged = merge_state(&known, &remote);
            if merged == known && !force {
                continue;
            }
            // Fields whose server value is new to this device.
            let fresh: BTreeSet<String> = merged
                .fields
                .iter()
                .filter(|(name, f)| force || known.fields.get(*name).is_none_or(|k| k.h != f.h))
                .map(|(name, _)| name.clone())
                .collect();
            let deleted_changed = force || merged.deleted != known.deleted;
            // The server's id may be a canonical one: find this device's row (sync_ids.rs).
            let local_id = self
                .resolve_local(&mut tx, &row.table, &row.row_id, &merged)
                .await?;
            let local_row = RemoteRow {
                row_id: local_id.clone(),
                ..row.clone()
            };
            // Unpushed local changes to this row decide which incoming fields lose.
            let pending = load_pending(&mut tx, &row.table, &local_id, &known, self.ctx().shared).await?;

            let mut savepoint = tx.begin().await?;
            match self
                .write_typed(
                    &mut savepoint,
                    &local_row,
                    &merged,
                    &fresh,
                    deleted_changed,
                    &pending,
                )
                .await
            {
                Ok(written) => {
                    savepoint.commit().await?;
                    remember(&mut tx, &row.table, &local_id, &row.row_id).await?;
                    pending.follow(&mut tx, &written.values).await?;
                    sqlx::query("DELETE FROM sync_conflict WHERE table_name = ? AND row_id = ?")
                        .bind(&row.table)
                        .bind(&row.row_id)
                        .execute(&mut *tx)
                        .await?;
                    if written.changed {
                        report.applied += 1;
                        // Everything a day's work-time result is computed from.
                        days_changed |= matches!(
                            row.table.as_str(),
                            "time_entry"
                                | "pause"
                                | "absence"
                                | "absence_type"
                                | "work_schedule"
                                | "work_settings"
                                | "jurisdiction_selection"
                                | "custom_holiday"
                                | "balance_adjustment"
                        );
                    }
                }
                Err(err) => {
                    savepoint.rollback().await?;
                    report.conflicts += 1;
                    sqlx::query(
                        "INSERT INTO sync_conflict (table_name, row_id, message, at) VALUES (?, ?, ?, ?) \
                         ON CONFLICT (table_name, row_id) DO UPDATE SET message = excluded.message, at = excluded.at",
                    )
                    .bind(&row.table)
                    .bind(&row.row_id)
                    .bind(friendly_conflict(&err.to_string()))
                    .bind(self.now_ms())
                    .execute(&mut *tx)
                    .await?;
                }
            }

            sqlx::query(
                "INSERT INTO sync_state (table_name, row_id, state) VALUES (?, ?, ?) \
                 ON CONFLICT (table_name, row_id) DO UPDATE SET state = excluded.state",
            )
            .bind(&row.table)
            .bind(&row.row_id)
            .bind(serde_json::to_string(&merged)?)
            .execute(&mut *tx)
            .await?;
        }

        if days_changed {
            // Work-time results of the touched days are stale.
            sqlx::query("DELETE FROM daily_summary").execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok((report, newest_hlc))
    }

    /// This device's id for a row the server names `canonical`. Rows that
    /// every device creates for itself (see `sync_ids.rs`) are found by what
    /// identifies them; a row new to this device gets the canonical id.
    async fn resolve_local(
        &self,
        conn: &mut SqliteConnection,
        table: &str,
        canonical: &str,
        merged: &RowState,
    ) -> Result<String> {
        if let Some(local) = local_of_canonical(conn, table, canonical).await? {
            return Ok(local);
        }
        let by_field = match rule(table) {
            Rule::Plain => return Ok(canonical.to_string()),
            Rule::Singleton => None,
            Rule::ByField(field) => match merged.fields.get(field).and_then(|f| f.v.as_str()) {
                Some(value) => Some((snake(field), value.to_string())),
                None => return Ok(canonical.to_string()),
            },
        };
        let cols = columns(conn, table).await?;
        let mut sql = format!("SELECT id FROM {table} WHERE deleted_at IS NULL");
        if cols.contains("workspace_id") {
            sql.push_str(" AND workspace_id = ?");
        }
        if cols.contains("user_id") && cols.not_null("user_id") {
            sql.push_str(" AND user_id = ?");
        }
        if by_field.is_some() {
            // The column name comes from the rule table, not from the server.
            sql.push_str(&format!(
                " AND {} = ?",
                by_field.as_ref().map(|(c, _)| c.as_str()).unwrap_or("id")
            ));
        }
        sql.push_str(" ORDER BY created_at LIMIT 1");
        let mut query = sqlx::query_scalar(sqlx::AssertSqlSafe(sql));
        if cols.contains("workspace_id") {
            query = query.bind(self.ctx().workspace_id.clone());
        }
        if cols.contains("user_id") && cols.not_null("user_id") {
            query = query.bind(self.ctx().user_id.clone());
        }
        if let Some((_, value)) = by_field {
            query = query.bind(value);
        }
        let local: Option<String> = query.fetch_optional(&mut *conn).await?;
        Ok(local.unwrap_or_else(|| canonical.to_string()))
    }

    /// Writes one row to its typed table. Returns whether anything changed.
    async fn write_typed(
        &self,
        conn: &mut SqliteConnection,
        row: &RemoteRow,
        merged: &RowState,
        fresh: &BTreeSet<String>,
        deleted_changed: bool,
        pending: &Pending,
    ) -> Result<Written> {
        let table = row.table.as_str();
        let cols = columns(conn, table).await?;
        let exists: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) > 0 FROM {table} WHERE id = ?"
        )))
        .bind(&row.row_id)
        .fetch_one(&mut *conn)
        .await?;

        // A row deleted on the server still takes the newer field values:
        // an edit that comes later may bring it back to life, and the fields
        // it did not touch must be current by then.
        let live = merged.is_live();
        if !live && (!exists || !cols.contains("deleted_at")) {
            return Ok(Written::default());
        }
        let deleted_at = merged
            .deleted
            .as_deref()
            .and_then(|h| hlc::decode(h).ok())
            .map_or(0, |(ts, _)| ts.wall_ms);

        // Fields to write: everything for a row new to this device, otherwise
        // the new ones that no newer local edit protects.
        let write: BTreeMap<String, &Versioned> = merged
            .fields
            .iter()
            .filter(|(name, f)| {
                if is_local_field(name, self.ctx().shared) {
                    return false;
                }
                if !exists {
                    return true;
                }
                // A field the user changed here, after the incoming write, stays.
                fresh.contains(*name) && !pending.protects(name, &f.h)
            })
            .map(|(name, f)| (name.clone(), f))
            .collect();
        if write.is_empty() && exists && !(!live && deleted_changed) {
            return Ok(Written::default());
        }
        // Deleted here after everything the server sent: the delete wins.
        if exists && live {
            if let (Some(deleted), Some(newest_remote)) = (&pending.delete, newest(&write)) {
                if *deleted > newest_remote {
                    return Ok(Written::default());
                }
            }
        }

        let mut values: Vec<(String, Bind)> = Vec::new();
        if matches!(table, "work_settings" | "display_settings" | "org_policy") {
            // One JSON column holds all the settings. Start from the local
            // ones and lay the incoming fields over them, so a field a local
            // edit protects keeps its value.
            let mut settings: serde_json::Map<String, Value> = if exists {
                let text: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                    "SELECT settings_json FROM {table} WHERE id = ?"
                )))
                .bind(&row.row_id)
                .fetch_optional(&mut *conn)
                .await?;
                text.and_then(|t| serde_json::from_str(&t).ok())
                    .unwrap_or_default()
            } else {
                Default::default()
            };
            for (name, field) in &write {
                // A policy's scope is a column of its own.
                if table == "org_policy" && name == "groupPath" {
                    if let Some(bind) = bind_of(&field.v) {
                        values.push(("group_path".into(), bind));
                    }
                    continue;
                }
                settings.insert(name.clone(), field.v.clone());
            }
            values.push((
                "settings_json".into(),
                Bind::Text(Value::Object(settings).to_string()),
            ));
        } else {
            for (name, field) in &write {
                // A group field stands for several columns (sync_groups.rs).
                if let Some(parts) = expand(table, name, &field.v) {
                    for (member, value) in parts {
                        let column = snake(&member);
                        if cols.contains(&column) {
                            if let Some(bind) = bind_of(&value) {
                                values.push((column, bind));
                            }
                        }
                    }
                    continue;
                }
                if table == "work_schedule" && name == "days" {
                    values.push(("days_json".into(), Bind::Text(field.v.to_string())));
                    continue;
                }
                if table == "project" && name == "memberIds" {
                    values.push(("member_ids_json".into(), Bind::Text(field.v.to_string())));
                    continue;
                }
                if table == "meeting" && name == "attendeeIds" {
                    values.push(("attendee_ids_json".into(), Bind::Text(field.v.to_string())));
                    continue;
                }
                let column = snake(name);
                if column == "id" || !cols.contains(&column) {
                    continue;
                }
                // References to rows that have other ids on this device.
                let reference = REFERENCES
                    .iter()
                    .find(|(owner, f, _)| *owner == table && f == name);
                let value = match (reference, &field.v) {
                    (Some((_, _, target)), Value::String(canonical)) => {
                        Value::String(reference_in(conn, target, canonical).await?)
                    }
                    (_, other) => other.clone(),
                };
                if let Some(bind) = bind_of(&value) {
                    values.push((column, bind));
                }
            }
        }
        let stamp =
            newest(&write).or_else(|| newest(&merged.fields.iter().map(|(k, v)| (k.clone(), v)).collect()));
        if let Some(h) = &stamp {
            if cols.contains("hlc") {
                values.push(("hlc".into(), Bind::Text(h.clone())));
            }
            if cols.contains("origin_device") {
                if let Ok((_, node)) = hlc::decode(h) {
                    values.push(("origin_device".into(), Bind::Text(node.to_string())));
                }
            }
        }
        if cols.contains("deleted_at") {
            values.push((
                "deleted_at".into(),
                if live { Bind::Null } else { Bind::Int(deleted_at) },
            ));
        }
        if cols.contains("workspace_id") {
            values.push(("workspace_id".into(), Bind::Text(self.ctx().workspace_id.clone())));
        }
        // Rows of this user. (The country selection's user is NULL: it is the
        // workspace default.)
        // (In a shared workspace the row's own user came with it.)
        if cols.contains("user_id") && cols.not_null("user_id") && !values.iter().any(|(c, _)| c == "user_id")
        {
            values.push(("user_id".into(), Bind::Text(self.ctx().user_id.clone())));
        }
        // Other people's rows name users this device has not met.
        if let Some((_, Bind::Text(owner))) = values.iter().find(|(c, _)| c == "user_id") {
            if self.ctx().shared && *owner != self.ctx().user_id {
                let now = self.now_ms();
                sqlx::query(
                    "INSERT OR IGNORE INTO app_user (id, display_name, created_at, updated_at, hlc, origin_device)
                     VALUES (?, 'Member', ?, ?, ?, 'server')",
                )
                .bind(owner)
                .bind(now)
                .bind(now)
                .bind(format!("{now:015}-000000-server"))
                .execute(&mut *conn)
                .await?;
            }
        }
        if !exists {
            // Rows from older clients may lack the stamps NOT NULL columns need.
            for stamp_col in ["created_at", "updated_at"] {
                if cols.contains(stamp_col) && !values.iter().any(|(c, _)| c == stamp_col) {
                    values.push((stamp_col.into(), Bind::Int(self.now_ms())));
                }
            }
        }

        let names: Vec<&str> = values.iter().map(|(c, _)| c.as_str()).collect();
        // Table and column names come from the table's own schema, not from
        // the server. An existing row is updated in place: a partial insert
        // would trip NOT NULL before the conflict clause is reached.
        let sql = if exists {
            format!(
                "UPDATE {table} SET {sets} WHERE id = ?",
                sets = names
                    .iter()
                    .map(|c| format!("{c} = ?"))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        } else {
            format!(
                "INSERT INTO {table} (id, {cols}) VALUES (?, {marks})",
                cols = names.join(", "),
                marks = vec!["?"; names.len()].join(", "),
            )
        };
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
        if !exists {
            query = query.bind(&row.row_id);
        }
        for (_, bind) in values {
            query = match bind {
                Bind::Null => query.bind(None::<i64>),
                Bind::Int(i) => query.bind(i),
                Bind::Real(f) => query.bind(f),
                Bind::Text(s) => query.bind(s),
            };
        }
        if exists {
            query = query.bind(&row.row_id);
        }
        query.execute(&mut *conn).await?;

        if table == "time_entry" {
            if let Some(Value::Array(ids)) = write.get("tagIds").map(|f| &f.v) {
                sqlx::query("DELETE FROM time_entry_tag WHERE entry_id = ?")
                    .bind(&row.row_id)
                    .execute(&mut *conn)
                    .await?;
                for id in ids.iter().filter_map(Value::as_str) {
                    sqlx::query("INSERT OR IGNORE INTO time_entry_tag (entry_id, tag_id) VALUES (?, ?)")
                        .bind(&row.row_id)
                        .bind(id)
                        .execute(&mut *conn)
                        .await?;
                }
            }
        }
        Ok(Written {
            changed: true,
            // The unpushed payloads hold single fields, not groups.
            values: write
                .iter()
                .flat_map(|(name, f)| {
                    expand(table, name, &f.v).unwrap_or_else(|| vec![(name.clone(), f.v.clone())])
                })
                .collect(),
        })
    }

    /// Remote changes that could not be written, newest first.
    pub async fn sync_conflicts(&self) -> Result<Vec<SyncConflict>> {
        let rows = sqlx::query(
            "SELECT c.table_name, c.row_id, c.message, c.at, s.state \
             FROM sync_conflict c LEFT JOIN sync_state s \
               ON s.table_name = c.table_name AND s.row_id = c.row_id \
             ORDER BY c.at DESC, c.id DESC",
        )
        .fetch_all(self.pool())
        .await?;
        rows.into_iter()
            .map(|r| {
                let state: Option<String> = r.get("state");
                let name = state
                    .and_then(|t| serde_json::from_str::<RowState>(&t).ok())
                    .and_then(|s| s.fields.get("name").and_then(|f| f.v.as_str().map(String::from)));
                Ok(SyncConflict {
                    table: r.get("table_name"),
                    row_id: r.get("row_id"),
                    name,
                    message: r.get("message"),
                    at: r.get("at"),
                })
            })
            .collect()
    }

    /// The account's version of a row next to this device's, for the conflict
    /// list. Fields that only identify the row on one device are left out.
    pub async fn sync_conflict_compare(&self, table: &str, row_id: &str) -> Result<Vec<FieldView>> {
        if !SYNCED_TABLES.contains(&table) {
            return Ok(Vec::new());
        }
        let mut conn = self.pool().acquire().await?;
        let state = load_state(&mut conn, table, row_id).await?;
        let local_id = self.resolve_local(&mut conn, table, row_id, &state).await?;
        let local = local_row_fields(&mut conn, table, &local_id).await?;

        let mut fields: Vec<(String, Value)> = Vec::new();
        for (name, versioned) in &state.fields {
            if is_local_field(name, self.ctx().shared) || matches!(name.as_str(), "createdAt" | "updatedAt") {
                continue;
            }
            match expand(table, name, &versioned.v) {
                Some(parts) => fields.extend(parts),
                None => fields.push((name.clone(), versioned.v.clone())),
            }
        }
        Ok(fields
            .into_iter()
            .map(|(field, server)| {
                let local = local
                    .as_ref()
                    .map(|row| row.get(&field).cloned().unwrap_or(Value::Null));
                FieldView { field, server, local }
            })
            .collect())
    }

    /// Tries to write the server's version of a conflicting row again, for
    /// example after the user changed the local row that was in the way.
    /// Returns whether it was written.
    pub async fn sync_retry_conflict(&self, table: &str, row_id: &str) -> Result<bool> {
        let mut conn = self.pool().acquire().await?;
        let state = load_state(&mut conn, table, row_id).await?;
        let row = RemoteRow {
            workspace_id: self.ctx().workspace_id.clone(),
            table: table.to_string(),
            row_id: row_id.to_string(),
            fields: state.fields,
            deleted: state.deleted,
        };
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;
        let result = self.apply_rows(&mut conn, &[row], true).await;
        let _ = sqlx::query("PRAGMA foreign_keys = ON").execute(&mut *conn).await;
        Ok(result?.0.conflicts == 0)
    }

    /// Forgets a conflict without applying the server's version; the local
    /// row stays as it is until the next change to it on either side.
    pub async fn sync_dismiss_conflict(&self, table: &str, row_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM sync_conflict WHERE table_name = ? AND row_id = ?")
            .bind(table)
            .bind(row_id)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    /// Remote changes waiting for the user to resolve.
    pub async fn sync_conflict_count(&self) -> Result<i64> {
        Ok(sqlx::query_scalar("SELECT count(*) FROM sync_conflict")
            .fetch_one(self.pool())
            .await?)
    }

    /// Throws away this device's copy of the synced data and the sync
    /// bookkeeping, so the next pull rebuilds it from the server. For a
    /// device that joins an account that already has data and should take
    /// the server's version rather than merge two histories. The workspace
    /// and user records stay.
    pub async fn sync_wipe_local(&self) -> Result<()> {
        let mut conn = self.pool().acquire().await?;
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await?;
        let result = async {
            let mut tx = conn.begin().await?;
            sqlx::query("DELETE FROM time_entry_tag")
                .execute(&mut *tx)
                .await?;
            for table in SYNCED_TABLES {
                // Names come from SYNCED_TABLES.
                sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM {table}")))
                    .execute(&mut *tx)
                    .await?;
                sqlx::query("DELETE FROM change_log WHERE table_name = ?")
                    .bind(table)
                    .execute(&mut *tx)
                    .await?;
            }
            sqlx::query("DELETE FROM sync_state").execute(&mut *tx).await?;
            sqlx::query("DELETE FROM sync_alias").execute(&mut *tx).await?;
            sqlx::query("DELETE FROM sync_conflict").execute(&mut *tx).await?;
            sqlx::query("DELETE FROM daily_summary").execute(&mut *tx).await?;
            sqlx::query(
                "INSERT INTO local_meta (key, value) VALUES ('sync_cursor', '0') \
                 ON CONFLICT (key) DO UPDATE SET value = '0'",
            )
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok::<_, crate::Error>(())
        }
        .await;
        let _ = sqlx::query("PRAGMA foreign_keys = ON").execute(&mut *conn).await;
        result
    }

    /// Position in the server's change feed.
    pub async fn sync_cursor(&self) -> Result<i64> {
        Ok(self
            .get_meta("sync_cursor")
            .await?
            .and_then(|c| c.parse().ok())
            .unwrap_or(0))
    }

    pub async fn set_sync_cursor(&self, cursor: i64) -> Result<()> {
        self.set_meta("sync_cursor", &cursor.to_string()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_database_errors_become_advice() {
        let raw = "error returned from database: (code: 1) no such column: nothing";
        // The index name is what SQLite reports for partial unique indexes.
        assert!(
            friendly_conflict("UNIQUE constraint failed: index 'time_entry_one_running'")
                .contains("timer is already running")
        );
        assert_eq!(friendly_conflict(raw), raw, "unknown errors stay as they are");
    }
}
