//! Timer engine: start / switch / stop / pause / resume.
//!
//! Each operation runs in one transaction with a single `now`, so switching
//! projects leaves no gap between the stopped and the started entry.

use sqlx::sqlite::SqliteRow;
use sqlx::{Row, Sqlite};

use crate::db::{Core, WriteTx};
use crate::model::{EntryInput, EntrySource, Pause, PauseKind, Project, StartTimer, TimerState};
use crate::repo::{fetch_entry, fetch_running, insert_entry, new_entry, update_entry_row};
use crate::{Error, Result};

macro_rules! pause_columns {
    () => {
        "id, workspace_id, user_id, start_at, end_at, kind, resume_entry_id, followup_entry_id, \
         applied, accepted, created_at, updated_at"
    };
}

fn pause_from_row(r: &SqliteRow) -> Result<Pause> {
    Ok(Pause {
        id: r.try_get("id")?,
        workspace_id: r.try_get("workspace_id")?,
        user_id: r.try_get("user_id")?,
        start_at: r.try_get("start_at")?,
        end_at: r.try_get("end_at")?,
        kind: r.try_get::<String, _>("kind")?.parse()?,
        resume_entry_id: r.try_get("resume_entry_id")?,
        followup_entry_id: r.try_get("followup_entry_id")?,
        applied: r.try_get("applied")?,
        accepted: r.try_get("accepted")?,
        created_at: r.try_get("created_at")?,
        updated_at: r.try_get("updated_at")?,
    })
}

async fn fetch_open_pause<'e, E>(executor: E, user_id: &str) -> Result<Option<Pause>>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let row = sqlx::query(concat!(
        "SELECT ",
        pause_columns!(),
        " FROM pause WHERE user_id = ? AND end_at IS NULL AND deleted_at IS NULL"
    ))
    .bind(user_id)
    .fetch_optional(executor)
    .await?;
    row.as_ref().map(pause_from_row).transpose()
}

pub(crate) async fn fetch_pause(conn: &mut sqlx::SqliteConnection, user_id: &str, id: &str) -> Result<Pause> {
    let row = sqlx::query(concat!(
        "SELECT ",
        pause_columns!(),
        " FROM pause WHERE id = ? AND user_id = ? AND deleted_at IS NULL"
    ))
    .bind(id)
    .bind(user_id)
    .fetch_optional(conn)
    .await?
    .ok_or_else(|| Error::NotFound {
        entity: "pause",
        id: id.to_string(),
    })?;
    pause_from_row(&row)
}

/// Auto-pauses waiting for a decision.
pub(crate) async fn list_pending_pauses<'e, E>(executor: E, user_id: &str) -> Result<Vec<Pause>>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let rows = sqlx::query(concat!(
        "SELECT ",
        pause_columns!(),
        " FROM pause WHERE user_id = ? AND accepted IS NULL AND kind != 'manual' AND deleted_at IS NULL
          ORDER BY start_at"
    ))
    .bind(user_id)
    .fetch_all(executor)
    .await?;
    rows.iter().map(pause_from_row).collect()
}

pub(crate) async fn insert_pause(tx: &mut WriteTx<'_>, p: &Pause) -> Result<()> {
    let s = tx.stamp();
    sqlx::query(
        "INSERT INTO pause (id, workspace_id, user_id, start_at, end_at, kind, resume_entry_id,
            followup_entry_id, applied, accepted, created_at, updated_at, hlc, origin_device)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&p.id)
    .bind(&p.workspace_id)
    .bind(&p.user_id)
    .bind(p.start_at)
    .bind(p.end_at)
    .bind(p.kind.as_str())
    .bind(&p.resume_entry_id)
    .bind(&p.followup_entry_id)
    .bind(p.applied)
    .bind(p.accepted)
    .bind(p.created_at)
    .bind(p.updated_at)
    .bind(&s.hlc)
    .bind(&s.device)
    .execute(tx.conn())
    .await?;
    tx.log_upsert("pause", &p.id, &s, p).await
}

pub(crate) async fn update_pause(tx: &mut WriteTx<'_>, p: &Pause) -> Result<()> {
    let s = tx.stamp();
    let p = Pause {
        updated_at: s.now,
        ..p.clone()
    };
    sqlx::query(
        "UPDATE pause SET start_at = ?, end_at = ?, followup_entry_id = ?, applied = ?, accepted = ?,
            updated_at = ?, hlc = ?, origin_device = ?
         WHERE id = ? AND user_id = ?",
    )
    .bind(p.start_at)
    .bind(p.end_at)
    .bind(&p.followup_entry_id)
    .bind(p.applied)
    .bind(p.accepted)
    .bind(p.updated_at)
    .bind(&s.hlc)
    .bind(&s.device)
    .bind(&p.id)
    .bind(&p.user_id)
    .execute(tx.conn())
    .await?;
    tx.log_upsert("pause", &p.id, &s, &p).await
}

impl Core {
    pub async fn timer_state(&self) -> Result<TimerState> {
        let user = &self.ctx().user_id;
        if let Some(entry) = fetch_running(self.pool(), user).await? {
            return Ok(TimerState::Running { entry });
        }
        if let Some(pause) = fetch_open_pause(self.pool(), user).await? {
            let resume_entry = match &pause.resume_entry_id {
                Some(id) => fetch_entry(self.pool(), user, id).await.ok(),
                None => None,
            };
            return Ok(TimerState::Paused { pause, resume_entry });
        }
        Ok(TimerState::Idle)
    }

    /// Starts tracking. A running entry is stopped and an open pause is
    /// closed at the same instant.
    pub async fn start_timer(&self, start: StartTimer) -> Result<TimerState> {
        let mut tx = self.begin().await?;
        stop_running(&mut tx).await?;
        close_pause(&mut tx).await?;
        start_entry(&mut tx, start).await?;
        tx.commit().await?;
        self.timer_state().await
    }

    pub async fn stop_timer(&self) -> Result<TimerState> {
        let mut tx = self.begin().await?;
        stop_running(&mut tx).await?;
        close_pause(&mut tx).await?;
        tx.commit().await?;
        self.timer_state().await
    }

    /// Stops the running entry and opens a pause that remembers it.
    pub async fn pause_timer(&self) -> Result<TimerState> {
        let mut tx = self.begin().await?;
        let Some(stopped) = stop_running(&mut tx).await? else {
            return Err(Error::Conflict("Nothing is being tracked.".into()));
        };
        let s = tx.stamp();
        let pause = Pause {
            id: crate::ids::new_id(),
            workspace_id: tx.ctx().workspace_id.clone(),
            user_id: tx.ctx().user_id.clone(),
            start_at: s.now,
            end_at: None,
            kind: PauseKind::Manual,
            resume_entry_id: Some(stopped),
            followup_entry_id: None,
            applied: true,
            accepted: Some(true),
            created_at: s.now,
            updated_at: s.now,
        };
        insert_pause(&mut tx, &pause).await?;
        tx.commit().await?;
        self.timer_state().await
    }

    /// Closes the open pause and continues the entry it interrupted with a
    /// new entry (same project, tags and note).
    pub async fn resume_timer(&self) -> Result<TimerState> {
        let mut tx = self.begin().await?;
        let Some(pause) = close_pause(&mut tx).await? else {
            return Err(Error::Conflict("The timer is not paused.".into()));
        };
        let user = tx.ctx().user_id.clone();
        let previous = match &pause.resume_entry_id {
            Some(id) => fetch_entry(tx.conn(), &user, id).await.ok(),
            None => None,
        };
        let start = match previous {
            Some(e) => StartTimer {
                project_id: e.project_id,
                tag_ids: e.tag_ids,
                note: e.note,
                billable: Some(e.billable),
            },
            None => StartTimer {
                project_id: None,
                tag_ids: vec![],
                note: String::new(),
                billable: None,
            },
        };
        start_entry(&mut tx, start).await?;
        tx.commit().await?;
        self.timer_state().await
    }

    /// Records that the app is alive while tracking, so a crash or power
    /// loss can be detected on next start.
    pub async fn heartbeat(&self) -> Result<()> {
        self.set_meta("timer_heartbeat_at", &self.now_ms().to_string())
            .await
    }

    pub async fn last_heartbeat(&self) -> Result<Option<i64>> {
        Ok(self
            .get_meta("timer_heartbeat_at")
            .await?
            .and_then(|v| v.parse().ok()))
    }

    /// Most recently tracked, still trackable projects (for the tray).
    pub async fn recent_projects(&self, limit: u32) -> Result<Vec<Project>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT e.project_id FROM time_entry e
               JOIN project p ON p.id = e.project_id
              WHERE e.user_id = ? AND e.deleted_at IS NULL
                AND p.deleted_at IS NULL AND p.status != 'archived'
              GROUP BY e.project_id
              ORDER BY max(e.start_at) DESC
              LIMIT ?",
        )
        .bind(&self.ctx().user_id)
        .bind(limit)
        .fetch_all(self.pool())
        .await?;
        let mut projects = Vec::with_capacity(ids.len());
        for id in ids {
            projects.push(self.get_project(&id).await?);
        }
        Ok(projects)
    }
}

/// Ends the running entry at `now`. Returns its id.
async fn stop_running(tx: &mut WriteTx<'_>) -> Result<Option<String>> {
    let user = tx.ctx().user_id.clone();
    let Some(entry) = fetch_running(tx.conn(), &user).await? else {
        return Ok(None);
    };
    let end = tx.now().max(entry.start_at);
    let id = entry.id.clone();
    let stopped = update_entry_row(
        tx,
        crate::model::TimeEntry {
            end_at: Some(end),
            ..entry
        },
    )
    .await?;
    tx.emit(crate::events::DomainEvent::EntryStopped { entry: stopped });
    Ok(Some(id))
}

/// Ends the open pause at `now`. Returns it.
async fn close_pause(tx: &mut WriteTx<'_>) -> Result<Option<Pause>> {
    let user = tx.ctx().user_id.clone();
    let Some(mut pause) = fetch_open_pause(tx.conn(), &user).await? else {
        return Ok(None);
    };
    let s = tx.stamp();
    pause.end_at = Some(s.now.max(pause.start_at));
    pause.updated_at = s.now;
    sqlx::query("UPDATE pause SET end_at = ?, updated_at = ?, hlc = ?, origin_device = ? WHERE id = ?")
        .bind(pause.end_at)
        .bind(pause.updated_at)
        .bind(&s.hlc)
        .bind(&s.device)
        .bind(&pause.id)
        .execute(tx.conn())
        .await?;
    tx.log_upsert("pause", &pause.id, &s, &pause).await?;
    Ok(Some(pause))
}

async fn start_entry(tx: &mut WriteTx<'_>, start: StartTimer) -> Result<()> {
    let input = EntryInput {
        project_id: start.project_id,
        start_at: tx.now(),
        end_at: None,
        tz: None,
        note: start.note,
        billable: start.billable,
        tag_ids: start.tag_ids,
        cost_center_id: None,
    }
    .normalized()?;
    let entry = new_entry(tx, input, EntrySource::Timer).await?;
    insert_entry(tx, &entry).await?;
    tx.emit(crate::events::DomainEvent::EntryStarted { entry });
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::clock::ManualClock;
    use crate::model::{BudgetPeriod, ProjectInput, ProjectStatus, TagInput};
    use crate::summary::Range;

    const MIN: i64 = 60_000;

    async fn setup() -> (Core, Arc<ManualClock>) {
        let clock = Arc::new(ManualClock::new(1_700_000_000_000));
        let core = Core::open_in_memory(clock.clone()).await.unwrap();
        (core, clock)
    }

    fn project(name: &str, alias: &str) -> ProjectInput {
        ProjectInput {
            name: name.into(),
            alias: Some(alias.into()),
            color: "#3b82f6".into(),
            parent_id: None,
            client: None,
            budget_minutes: None,
            budget_period: BudgetPeriod::Total,
            budget_warn_pct: 80,
            billable_default: true,
            status: ProjectStatus::Active,
            cost_center_id: None,
        }
    }

    fn start(project_id: Option<&str>) -> StartTimer {
        StartTimer {
            project_id: project_id.map(Into::into),
            tag_ids: vec![],
            note: String::new(),
            billable: None,
        }
    }

    #[tokio::test]
    async fn switching_projects_leaves_no_gap() {
        let (core, clock) = setup().await;
        let a = core.create_project(project("A", "A")).await.unwrap();
        let b = core.create_project(project("B", "B")).await.unwrap();

        core.start_timer(start(Some(&a.id))).await.unwrap();
        clock.advance(30 * MIN);
        let state = core.start_timer(start(Some(&b.id))).await.unwrap();
        let TimerState::Running { entry } = state else {
            panic!("expected running")
        };
        assert_eq!(entry.project_id.as_deref(), Some(b.id.as_str()));
        assert!(entry.billable, "billable defaults from the project");

        clock.advance(10 * MIN);
        core.stop_timer().await.unwrap();

        let entries = core
            .list_entries(Range {
                from: 0,
                to: i64::MAX,
            })
            .await
            .unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].end_at, Some(entries[1].start_at));
        assert_eq!(entries[0].end_at.unwrap() - entries[0].start_at, 30 * MIN);
        assert_eq!(entries[1].end_at.unwrap() - entries[1].start_at, 10 * MIN);
        assert_eq!(core.timer_state().await.unwrap(), TimerState::Idle);
    }

    #[tokio::test]
    async fn pause_and_resume_continue_the_same_activity() {
        let (core, clock) = setup().await;
        let a = core.create_project(project("A", "A")).await.unwrap();
        let tag = core
            .create_tag(TagInput {
                name: "Dev".into(),
                color: "#22c55e".into(),
                project_id: None,
                group_name: None,
            })
            .await
            .unwrap();
        core.start_timer(StartTimer {
            tag_ids: vec![tag.id.clone()],
            note: "feature".into(),
            ..start(Some(&a.id))
        })
        .await
        .unwrap();
        clock.advance(20 * MIN);

        let paused = core.pause_timer().await.unwrap();
        assert!(matches!(
            paused,
            TimerState::Paused {
                resume_entry: Some(_),
                ..
            }
        ));
        clock.advance(5 * MIN);

        let TimerState::Running { entry } = core.resume_timer().await.unwrap() else {
            panic!("expected running")
        };
        assert_eq!(entry.project_id.as_deref(), Some(a.id.as_str()));
        assert_eq!(entry.tag_ids, vec![tag.id]);
        assert_eq!(entry.note, "feature");

        let summary = core
            .summaries(vec![Range {
                from: 0,
                to: i64::MAX,
            }])
            .await
            .unwrap();
        assert_eq!(summary[0].total_ms, 20 * MIN, "pause time is not tracked");
    }

    #[tokio::test]
    async fn pause_without_running_timer_is_a_conflict() {
        let (core, _) = setup().await;
        assert!(matches!(core.pause_timer().await, Err(Error::Conflict(_))));
        assert!(matches!(core.resume_timer().await, Err(Error::Conflict(_))));
    }

    #[tokio::test]
    async fn archived_projects_cannot_be_tracked() {
        let (core, _) = setup().await;
        let p = core
            .create_project(ProjectInput {
                status: ProjectStatus::Archived,
                ..project("Old", "OLD")
            })
            .await
            .unwrap();
        assert!(matches!(
            core.start_timer(start(Some(&p.id))).await,
            Err(Error::Validation { .. })
        ));
    }

    #[tokio::test]
    async fn every_write_is_change_logged() {
        let (core, clock) = setup().await;
        let before = core.pending_changes().await.unwrap();
        let a = core.create_project(project("A", "A")).await.unwrap(); // +1
        core.start_timer(start(Some(&a.id))).await.unwrap(); // +1 insert
        clock.advance(MIN);
        core.stop_timer().await.unwrap(); // +1 update
        assert_eq!(core.pending_changes().await.unwrap() - before, 3);
    }

    #[tokio::test]
    async fn recent_projects_are_ordered_by_last_use() {
        let (core, clock) = setup().await;
        let a = core.create_project(project("A", "A")).await.unwrap();
        let b = core.create_project(project("B", "B")).await.unwrap();
        core.start_timer(start(Some(&a.id))).await.unwrap();
        clock.advance(MIN);
        core.start_timer(start(Some(&b.id))).await.unwrap();
        clock.advance(MIN);
        core.start_timer(start(Some(&a.id))).await.unwrap();
        let recent: Vec<_> = core
            .recent_projects(5)
            .await
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(recent, vec!["A", "B"]);
    }
}
