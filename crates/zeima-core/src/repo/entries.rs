use std::collections::HashMap;

use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite, SqliteConnection};

use super::activities::fetch_activity;
use super::{
    check_entry_activity, ensure_trackable, fetch_project, fetch_tag, invalidate_entry_days, not_found,
    resolve_cost_center, soft_delete, validate_entry_tags,
};
use crate::db::{Core, WriteTx};
use crate::events::DomainEvent;
use crate::model::{BulkResult, EntryInput, EntryPatch, EntrySource, Tag, TimeEntry};
use crate::report::{build_report, Report, ReportQuery};
use crate::summary::{summarize, Range, Summary};
use crate::{Error, Result};

/// Live tag ids are aggregated into a comma-separated column.
macro_rules! select {
    () => {
        "SELECT e.id, e.workspace_id, e.user_id, e.activity_id, e.project_id, e.start_at, e.end_at,
    e.tz, e.note, e.billable, e.source, e.source_ref, e.locked, e.cost_center_id,
    e.created_at, e.updated_at,
    (SELECT group_concat(et.tag_id) FROM time_entry_tag et
       JOIN tag t ON t.id = et.tag_id AND t.deleted_at IS NULL
      WHERE et.entry_id = e.id) AS tag_ids
    FROM time_entry e"
    };
}

fn from_row(r: &SqliteRow) -> Result<TimeEntry> {
    let tag_ids: Option<String> = r.try_get("tag_ids")?;
    let mut tag_ids: Vec<String> = tag_ids
        .map(|s| s.split(',').map(str::to_string).collect())
        .unwrap_or_default();
    tag_ids.sort();
    Ok(TimeEntry {
        id: r.try_get("id")?,
        workspace_id: r.try_get("workspace_id")?,
        user_id: r.try_get("user_id")?,
        activity_id: r.try_get("activity_id")?,
        project_id: r.try_get("project_id")?,
        start_at: r.try_get("start_at")?,
        end_at: r.try_get("end_at")?,
        tz: r.try_get("tz")?,
        note: r.try_get("note")?,
        billable: r.try_get("billable")?,
        source: r.try_get::<String, _>("source")?.parse()?,
        source_ref: r.try_get("source_ref")?,
        locked: r.try_get("locked")?,
        tag_ids,
        cost_center_id: r.try_get("cost_center_id")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

pub(crate) async fn fetch_entry<'e, E>(executor: E, user_id: &str, id: &str) -> Result<TimeEntry>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let row = sqlx::query(concat!(
        select!(),
        " WHERE e.id = ? AND e.user_id = ? AND e.deleted_at IS NULL"
    ))
    .bind(id)
    .bind(user_id)
    .fetch_optional(executor)
    .await?
    .ok_or_else(|| not_found("entry", id))?;
    from_row(&row)
}

pub(crate) async fn fetch_running<'e, E>(executor: E, user_id: &str) -> Result<Option<TimeEntry>>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let row = sqlx::query(concat!(
        select!(),
        " WHERE e.user_id = ? AND e.end_at IS NULL AND e.deleted_at IS NULL"
    ))
    .bind(user_id)
    .fetch_optional(executor)
    .await?;
    row.as_ref().map(from_row).transpose()
}

impl Core {
    /// Entries of the current user that overlap the range, by start time.
    pub async fn list_entries(&self, range: Range) -> Result<Vec<TimeEntry>> {
        let rows = sqlx::query(concat!(
            select!(),
            " WHERE e.user_id = ? AND e.deleted_at IS NULL
               AND e.start_at < ? AND (e.end_at IS NULL OR e.end_at > ?)
             ORDER BY e.start_at"
        ))
        .bind(&self.ctx().user_id)
        .bind(range.to)
        .bind(range.from)
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(from_row).collect()
    }

    pub async fn get_entry(&self, id: &str) -> Result<TimeEntry> {
        fetch_entry(self.pool(), &self.ctx().user_id, id).await
    }

    /// Summaries for several ranges (e.g. each day of a week) in one call.
    pub async fn summaries(&self, ranges: Vec<Range>) -> Result<Vec<Summary>> {
        let (Some(from), Some(to)) = (
            ranges.iter().map(|r| r.from).min(),
            ranges.iter().map(|r| r.to).max(),
        ) else {
            return Ok(vec![]);
        };
        let entries = self.list_entries(Range { from, to }).await?;
        let now = self.now_ms();
        Ok(ranges.into_iter().map(|r| summarize(&entries, r, now)).collect())
    }

    pub async fn report(&self, query: &ReportQuery) -> Result<Report> {
        let entries = self.list_entries(query.range).await?;
        Ok(build_report(
            &entries,
            query,
            self.now_ms(),
            self.tz()?,
            self.week_start(),
        ))
    }

    /// Manual entry. Use the timer to create a running entry.
    pub async fn create_entry(&self, input: EntryInput) -> Result<TimeEntry> {
        self.create_entry_from(input, EntrySource::Manual, None).await
    }

    /// Like [`Core::create_entry`], recording where the entry came from
    /// (e.g. an addon and its id).
    pub async fn create_entry_from(
        &self,
        input: EntryInput,
        source: EntrySource,
        source_ref: Option<String>,
    ) -> Result<TimeEntry> {
        let input = input.normalized()?;
        if input.end_at.is_none() {
            return Err(Error::validation(
                "endAt",
                "is required; use the timer for running entries",
            ));
        }
        let mut tx = self.begin().await?;
        let mut entry = new_entry(&mut tx, input, source).await?;
        entry.source_ref = source_ref;
        insert_entry(&mut tx, &entry).await?;
        tx.commit().await?;
        Ok(entry)
    }

    pub async fn update_entry(&self, id: &str, input: EntryInput) -> Result<TimeEntry> {
        let input = input.normalized()?;
        let mut tx = self.begin().await?;
        let user = tx.ctx().user_id.clone();
        let existing = fetch_entry(tx.conn(), &user, id).await?;
        ensure_unlocked(&existing)?;
        let entry = update_in(&mut tx, existing, input, true).await?;
        tx.commit().await?;
        Ok(entry)
    }

    /// Applies `patch` to the given entries in one transaction. Locked
    /// entries are skipped; any other problem rolls everything back.
    pub async fn bulk_update_entries(&self, ids: &[String], patch: EntryPatch) -> Result<BulkResult> {
        let mut tx = self.begin().await?;
        let user = tx.ctx().user_id.clone();
        let mut tags: HashMap<String, Tag> = HashMap::new();
        for id in patch.add_tag_ids.iter() {
            let tag = fetch_tag(tx.conn(), id)
                .await
                .map_err(|_| Error::validation("addTagIds", format!("unknown tag {id}")))?;
            tags.insert(id.clone(), tag);
        }
        let mut result = BulkResult::default();
        for id in ids {
            let e = fetch_entry(tx.conn(), &user, id).await?;
            if e.locked {
                result.skipped_locked += 1;
                continue;
            }
            for tag_id in &e.tag_ids {
                if !tags.contains_key(tag_id) {
                    if let Ok(tag) = fetch_tag(tx.conn(), tag_id).await {
                        tags.insert(tag_id.clone(), tag);
                    }
                }
            }
            let project_id = if patch.set_project {
                patch.project_id.clone()
            } else {
                e.project_id.clone()
            };
            let group = |id: &String| tags.get(id).and_then(|t| t.group_name.clone());
            let mut tag_ids: Vec<String> = e
                .tag_ids
                .iter()
                .filter(|id| !patch.remove_tag_ids.contains(id))
                .cloned()
                .collect();
            for added in &patch.add_tag_ids {
                if let Some(g) = group(added) {
                    tag_ids.retain(|id| id == added || group(id).as_deref() != Some(g.as_str()));
                }
                tag_ids.push(added.clone());
            }
            // Tags scoped to another project do not apply (also added ones).
            tag_ids.retain(|id| {
                tags.get(id)
                    .is_none_or(|t| t.project_id.is_none() || t.project_id == project_id)
            });
            let input = EntryInput {
                activity_id: patch.activity_id.clone().or_else(|| e.activity_id.clone()),
                cost_center_id: if project_id == e.project_id {
                    e.cost_center_id.clone()
                } else {
                    None
                },
                project_id,
                start_at: e.start_at,
                end_at: e.end_at,
                tz: Some(e.tz.clone()),
                note: e.note.clone(),
                billable: Some(patch.billable.unwrap_or(e.billable)),
                tag_ids,
            }
            .normalized()?;
            update_in(&mut tx, e, input, false).await?;
            result.changed += 1;
        }
        tx.commit().await?;
        Ok(result)
    }

    /// Deletes the given entries in one transaction; locked ones are skipped.
    pub async fn bulk_delete_entries(&self, ids: &[String]) -> Result<BulkResult> {
        let mut tx = self.begin().await?;
        let user = tx.ctx().user_id.clone();
        let mut result = BulkResult::default();
        for id in ids {
            let e = fetch_entry(tx.conn(), &user, id).await?;
            if e.locked {
                result.skipped_locked += 1;
                continue;
            }
            soft_delete(&mut tx, "time_entry", id).await?;
            invalidate_entry_days(&mut tx, e.start_at, e.end_at).await?;
            result.changed += 1;
        }
        tx.commit().await?;
        Ok(result)
    }

    pub async fn delete_entry(&self, id: &str) -> Result<()> {
        let mut tx = self.begin().await?;
        let user = tx.ctx().user_id.clone();
        let existing = fetch_entry(tx.conn(), &user, id).await?;
        ensure_unlocked(&existing)?;
        soft_delete(&mut tx, "time_entry", id).await?;
        invalidate_entry_days(&mut tx, existing.start_at, existing.end_at).await?;
        tx.commit().await
    }

    /// Splits an entry at `at` into two entries. A running entry keeps
    /// running in the second part.
    pub async fn split_entry(&self, id: &str, at: i64) -> Result<(TimeEntry, TimeEntry)> {
        let mut tx = self.begin().await?;
        let user = tx.ctx().user_id.clone();
        let existing = fetch_entry(tx.conn(), &user, id).await?;
        ensure_unlocked(&existing)?;
        let end = existing.end_at.unwrap_or(tx.now());
        if at <= existing.start_at || at >= end {
            return Err(Error::validation("at", "must be inside the entry"));
        }
        let second_template = existing.clone();
        let first = update_entry_row(
            &mut tx,
            TimeEntry {
                end_at: Some(at),
                ..existing
            },
        )
        .await?;
        let s = tx.stamp();
        let second = TimeEntry {
            id: crate::ids::new_id(),
            start_at: at,
            created_at: s.now,
            updated_at: s.now,
            ..second_template
        };
        insert_entry(&mut tx, &second).await?;
        tx.commit().await?;
        Ok((first, second))
    }

    /// Copies a finished entry to start right where it ended.
    pub async fn duplicate_entry(&self, id: &str) -> Result<TimeEntry> {
        let mut tx = self.begin().await?;
        let user = tx.ctx().user_id.clone();
        let existing = fetch_entry(tx.conn(), &user, id).await?;
        let Some(end) = existing.end_at else {
            return Err(Error::validation("id", "a running entry cannot be duplicated"));
        };
        let s = tx.stamp();
        let copy = TimeEntry {
            id: crate::ids::new_id(),
            start_at: end,
            end_at: Some(end + (end - existing.start_at)),
            source: EntrySource::Manual,
            source_ref: None,
            locked: false,
            created_at: s.now,
            updated_at: s.now,
            ..existing
        };
        insert_entry(&mut tx, &copy).await?;
        tx.commit().await?;
        Ok(copy)
    }
}

/// Validates `input` against the unlocked entry `existing` and writes it.
/// `require_activity = false` lets entries recorded before activities keep
/// having none (bulk changes of other fields).
async fn update_in(
    tx: &mut WriteTx<'_>,
    existing: TimeEntry,
    input: EntryInput,
    require_activity: bool,
) -> Result<TimeEntry> {
    if require_activity || input.activity_id.is_some() || existing.activity_id.is_some() {
        check_entry_activity(tx, input.activity_id.as_deref(), existing.activity_id.as_deref()).await?;
    }
    if !existing.is_running() && input.end_at.is_none() {
        return Err(Error::validation("endAt", "a finished entry needs an end"));
    }
    let billable = match input.billable {
        Some(b) => b,
        None => existing.billable,
    };
    let ws = tx.ctx().workspace_id.clone();
    if existing.project_id != input.project_id {
        if let Some(pid) = &input.project_id {
            ensure_trackable(&fetch_project(tx.conn(), &ws, pid).await?)?;
        }
    }
    validate_entry_tags(tx.conn(), &ws, input.project_id.as_deref(), &input.tag_ids).await?;
    let cost_center_id = resolve_cost_center(
        tx,
        input.cost_center_id.as_deref(),
        input.project_id.as_deref(),
        input.start_at,
    )
    .await?;
    let entry = TimeEntry {
        activity_id: input.activity_id,
        project_id: input.project_id,
        start_at: input.start_at,
        end_at: input.end_at,
        tz: input.tz.unwrap_or(existing.tz.clone()),
        note: input.note,
        billable,
        tag_ids: input.tag_ids,
        cost_center_id,
        ..existing
    };
    update_entry_row(tx, entry).await
}

fn ensure_unlocked(entry: &TimeEntry) -> Result<()> {
    if entry.locked {
        return Err(Error::Conflict("This entry is locked.".into()));
    }
    Ok(())
}

/// Builds a new entry from validated input (activity, project and tags
/// checked, billable defaulted from the project). Manual entries need an
/// activity; the timer checks it in [`Core::start_timer`].
pub(crate) async fn new_entry(
    tx: &mut WriteTx<'_>,
    input: EntryInput,
    source: EntrySource,
) -> Result<TimeEntry> {
    let ws = tx.ctx().workspace_id.clone();
    if source == EntrySource::Manual {
        check_entry_activity(tx, input.activity_id.as_deref(), None).await?;
    } else if let Some(id) = &input.activity_id {
        fetch_activity(tx.conn(), &ws, id)
            .await
            .map_err(|_| Error::validation("activityId", "activity not found"))?;
    }
    let mut billable_default = false;
    if let Some(pid) = &input.project_id {
        let project = fetch_project(tx.conn(), &ws, pid).await?;
        ensure_trackable(&project)?;
        billable_default = project.billable_default;
    }
    validate_entry_tags(tx.conn(), &ws, input.project_id.as_deref(), &input.tag_ids).await?;
    let cost_center_id = resolve_cost_center(
        tx,
        input.cost_center_id.as_deref(),
        input.project_id.as_deref(),
        input.start_at,
    )
    .await?;
    let now = tx.now();
    Ok(TimeEntry {
        id: crate::ids::new_id(),
        workspace_id: ws,
        user_id: tx.ctx().user_id.clone(),
        activity_id: input.activity_id,
        project_id: input.project_id,
        start_at: input.start_at,
        end_at: input.end_at,
        tz: input.tz.unwrap_or_else(|| tx.default_tz()),
        note: input.note,
        billable: input.billable.unwrap_or(billable_default),
        source,
        source_ref: None,
        locked: false,
        tag_ids: input.tag_ids,
        cost_center_id,
        created_at: now,
        updated_at: now,
    })
}

pub(crate) async fn insert_entry(tx: &mut WriteTx<'_>, e: &TimeEntry) -> Result<()> {
    let s = tx.stamp();
    sqlx::query(
        "INSERT INTO time_entry (id, workspace_id, user_id, activity_id, project_id, start_at, end_at, tz,
            note, billable, source, source_ref, locked, cost_center_id, created_at, updated_at, hlc,
            origin_device)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&e.id)
    .bind(&e.workspace_id)
    .bind(&e.user_id)
    .bind(&e.activity_id)
    .bind(&e.project_id)
    .bind(e.start_at)
    .bind(e.end_at)
    .bind(&e.tz)
    .bind(&e.note)
    .bind(e.billable)
    .bind(e.source.as_str())
    .bind(&e.source_ref)
    .bind(e.locked)
    .bind(&e.cost_center_id)
    .bind(e.created_at)
    .bind(e.updated_at)
    .bind(&s.hlc)
    .bind(&s.device)
    .execute(tx.conn())
    .await
    .map_err(running_conflict)?;
    write_tags(tx.conn(), &e.id, &e.tag_ids).await?;
    invalidate_entry_days(tx, e.start_at, e.end_at).await?;
    tx.emit(DomainEvent::EntryCreated { entry: e.clone() });
    tx.log_upsert("time_entry", &e.id, &s, e).await
}

/// Writes all mutable fields of an existing entry and returns it with the
/// new `updated_at`.
pub(crate) async fn update_entry_row(tx: &mut WriteTx<'_>, mut e: TimeEntry) -> Result<TimeEntry> {
    let old: Option<(i64, Option<i64>)> =
        sqlx::query_as("SELECT start_at, end_at FROM time_entry WHERE id = ? AND user_id = ?")
            .bind(&e.id)
            .bind(&e.user_id)
            .fetch_optional(tx.conn())
            .await?;
    if let Some((start, end)) = old {
        invalidate_entry_days(tx, start, end).await?;
    }
    invalidate_entry_days(tx, e.start_at, e.end_at).await?;
    let s = tx.stamp();
    e.updated_at = s.now;
    sqlx::query(
        "UPDATE time_entry SET activity_id = ?, project_id = ?, start_at = ?, end_at = ?, tz = ?, note = ?,
            billable = ?, locked = ?, cost_center_id = ?, updated_at = ?, hlc = ?, origin_device = ?
         WHERE id = ? AND user_id = ?",
    )
    .bind(&e.activity_id)
    .bind(&e.project_id)
    .bind(e.start_at)
    .bind(e.end_at)
    .bind(&e.tz)
    .bind(&e.note)
    .bind(e.billable)
    .bind(e.locked)
    .bind(&e.cost_center_id)
    .bind(e.updated_at)
    .bind(&s.hlc)
    .bind(&s.device)
    .bind(&e.id)
    .bind(&e.user_id)
    .execute(tx.conn())
    .await
    .map_err(running_conflict)?;
    sqlx::query("DELETE FROM time_entry_tag WHERE entry_id = ?")
        .bind(&e.id)
        .execute(tx.conn())
        .await?;
    write_tags(tx.conn(), &e.id, &e.tag_ids).await?;
    tx.log_upsert("time_entry", &e.id, &s, &e).await?;
    tx.emit(DomainEvent::EntryUpdated { entry: e.clone() });
    Ok(e)
}

async fn write_tags(conn: &mut SqliteConnection, entry_id: &str, tag_ids: &[String]) -> Result<()> {
    for tag_id in tag_ids {
        sqlx::query("INSERT INTO time_entry_tag (entry_id, tag_id) VALUES (?, ?)")
            .bind(entry_id)
            .bind(tag_id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

fn running_conflict(err: sqlx::Error) -> Error {
    match &err {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            Error::Conflict("Another entry is already running.".into())
        }
        _ => Error::Db(err),
    }
}
