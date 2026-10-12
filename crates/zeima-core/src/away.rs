//! Applies auto-pause decisions to the timer and lets the user resolve
//! them afterwards (keep excluded, count as work, assign to a project).

use serde::{Deserialize, Serialize};

use crate::autopause::{decide, AutoPauseConfig, AwayCause, AwayPeriod, Decision};
use crate::db::{Core, WriteTx};
use crate::model::{EntrySource, Pause, PauseKind, TimeEntry};
use crate::repo::{
    ensure_trackable, fetch_entry, fetch_project, fetch_running, insert_entry, resolve_cost_center,
    soft_delete, update_entry_row,
};
use crate::timer::{fetch_pause, insert_pause, list_pending_pauses, update_pause};
use crate::{Error, Result};

/// The app was not running for longer than this while a timer ran.
const FORGOTTEN_AFTER_MS: i64 = 2 * 60_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct AwayItem {
    pub pause: Pause,
    /// The entry that was running when the user went away.
    pub entry: Option<TimeEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AwayResolution {
    /// Not work: keep (or make) the gap.
    Exclude,
    /// It was work: undo the pause.
    Keep,
    /// It was work on another project.
    #[serde(rename_all = "camelCase")]
    Assign { project_id: String },
}

impl Core {
    /// Decides what to do with an away period and applies it to the running
    /// entry. Returns the recorded pause, if any.
    pub async fn handle_away(&self, period: AwayPeriod, cfg: &AutoPauseConfig) -> Result<Option<Pause>> {
        let decision = decide(period, cfg, self.tz()?);
        if decision == Decision::Ignore {
            return Ok(None);
        }
        let mut tx = self.begin().await?;
        let user = tx.ctx().user_id.clone();
        let Some(running) = fetch_running(tx.conn(), &user).await? else {
            return Ok(None);
        };
        let from = period.from.max(running.start_at);
        if from >= period.to {
            return Ok(None);
        }
        let (kind, applied, followup) = match decision {
            Decision::Ignore => return Ok(None),
            Decision::StopAtStart => {
                update_entry_row(
                    &mut tx,
                    TimeEntry {
                        end_at: Some(from),
                        ..running.clone()
                    },
                )
                .await?;
                (PauseKind::AutoNight, true, None)
            }
            Decision::Exclude(kind) => {
                let followup = split_out(&mut tx, running.clone(), from, period.to).await?;
                (kind, true, followup)
            }
            Decision::Prompt(kind) => (kind, false, None),
        };
        let pause = new_pause(&tx, from, period.to, kind, &running.id, followup, applied);
        insert_pause(&mut tx, &pause).await?;
        tx.commit().await?;
        Ok(Some(pause))
    }

    /// On startup: a timer that kept "running" while the app was closed (or
    /// crashed) is treated like time away since the last heartbeat.
    pub async fn check_forgotten_timer(&self, cfg: &AutoPauseConfig) -> Result<Option<Pause>> {
        let Some(heartbeat) = self.last_heartbeat().await? else {
            return Ok(None);
        };
        let now = self.now_ms();
        if now - heartbeat < FORGOTTEN_AFTER_MS
            || fetch_running(self.pool(), &self.ctx().user_id).await?.is_none()
        {
            return Ok(None);
        }
        self.handle_away(
            AwayPeriod {
                from: heartbeat,
                to: now,
                cause: AwayCause::AppClosed,
            },
            cfg,
        )
        .await
    }

    /// Auto-pauses waiting for the user's decision, oldest first.
    pub async fn pending_away(&self) -> Result<Vec<AwayItem>> {
        let user = &self.ctx().user_id;
        let mut items = Vec::new();
        for pause in list_pending_pauses(self.pool(), user).await? {
            let entry = match &pause.resume_entry_id {
                Some(id) => fetch_entry(self.pool(), user, id).await.ok(),
                None => None,
            };
            items.push(AwayItem { pause, entry });
        }
        Ok(items)
    }

    pub async fn resolve_away(&self, pause_id: &str, resolution: AwayResolution) -> Result<()> {
        let mut tx = self.begin().await?;
        let user = tx.ctx().user_id.clone();
        let mut pause = fetch_pause(tx.conn(), &user, pause_id).await?;
        if pause.accepted.is_some() || pause.kind == PauseKind::Manual {
            return Err(Error::Conflict("This pause was already handled.".into()));
        }
        let to = pause.end_at.unwrap_or(pause.start_at);
        let entry = match &pause.resume_entry_id {
            Some(id) => fetch_entry(tx.conn(), &user, id).await.ok(),
            None => None,
        };

        match resolution {
            AwayResolution::Exclude => {
                apply_if_needed(&mut tx, &mut pause, entry).await?;
                pause.accepted = Some(true);
                update_pause(&mut tx, &pause).await?;
            }
            AwayResolution::Keep => {
                if pause.applied {
                    if let Some(entry) = entry {
                        undo(&mut tx, &pause, entry).await?;
                    }
                }
                soft_delete(&mut tx, "pause", &pause.id).await?;
            }
            AwayResolution::Assign { project_id } => {
                let ws = tx.ctx().workspace_id.clone();
                ensure_trackable(&fetch_project(tx.conn(), &ws, &project_id).await?)?;
                apply_if_needed(&mut tx, &mut pause, entry.clone()).await?;
                let cost_center_id =
                    resolve_cost_center(&mut tx, None, Some(&project_id), pause.start_at).await?;
                let gap = TimeEntry {
                    id: crate::ids::new_id(),
                    project_id: Some(project_id),
                    start_at: pause.start_at,
                    end_at: Some(to),
                    note: String::new(),
                    tag_ids: vec![],
                    source: EntrySource::Manual,
                    source_ref: None,
                    locked: false,
                    created_at: tx.now(),
                    updated_at: tx.now(),
                    billable: false,
                    cost_center_id,
                    ..entry
                        .ok_or_else(|| Error::Conflict("The interrupted entry no longer exists.".into()))?
                };
                insert_entry(&mut tx, &gap).await?;
                soft_delete(&mut tx, "pause", &pause.id).await?;
            }
        }
        tx.commit().await
    }
}

fn new_pause(
    tx: &WriteTx<'_>,
    from: i64,
    to: i64,
    kind: PauseKind,
    entry_id: &str,
    followup: Option<String>,
    applied: bool,
) -> Pause {
    Pause {
        id: crate::ids::new_id(),
        workspace_id: tx.ctx().workspace_id.clone(),
        user_id: tx.ctx().user_id.clone(),
        start_at: from,
        end_at: Some(to),
        kind,
        resume_entry_id: Some(entry_id.to_string()),
        followup_entry_id: followup,
        applied,
        accepted: None,
        created_at: tx.now(),
        updated_at: tx.now(),
    }
}

/// Cuts `[from, to)` out of `entry`: it ends at `from`, and a copy continues
/// at `to` until the original end (or keeps running). Returns the copy's id.
async fn split_out(tx: &mut WriteTx<'_>, entry: TimeEntry, from: i64, to: i64) -> Result<Option<String>> {
    let original_end = entry.end_at;
    if original_end.is_some_and(|end| end <= from) {
        return Ok(None);
    }
    update_entry_row(
        tx,
        TimeEntry {
            end_at: Some(from),
            ..entry.clone()
        },
    )
    .await?;
    if original_end.is_some_and(|end| end <= to) {
        return Ok(None);
    }
    let copy = TimeEntry {
        id: crate::ids::new_id(),
        start_at: to,
        end_at: original_end,
        created_at: tx.now(),
        updated_at: tx.now(),
        ..entry
    };
    insert_entry(tx, &copy).await?;
    Ok(Some(copy.id))
}

/// For a prompted pause that was not applied yet: cut it out now.
async fn apply_if_needed(tx: &mut WriteTx<'_>, pause: &mut Pause, entry: Option<TimeEntry>) -> Result<()> {
    if pause.applied {
        return Ok(());
    }
    if let Some(entry) = entry {
        let to = pause.end_at.unwrap_or(pause.start_at);
        if entry.start_at < pause.start_at {
            pause.followup_entry_id = split_out(tx, entry, pause.start_at, to).await?;
        }
    }
    pause.applied = true;
    Ok(())
}

/// Turns an applied pause back into work time.
async fn undo(tx: &mut WriteTx<'_>, pause: &Pause, entry: TimeEntry) -> Result<()> {
    let user = tx.ctx().user_id.clone();
    let to = pause.end_at.unwrap_or(pause.start_at);
    if pause.kind == PauseKind::AutoNight {
        // Continue the stopped timer, unless something else runs now.
        let running = fetch_running(tx.conn(), &user).await?;
        let end_at = if running.is_none() { None } else { Some(to) };
        update_entry_row(tx, TimeEntry { end_at, ..entry }).await?;
        return Ok(());
    }
    let followup = match &pause.followup_entry_id {
        Some(id) => fetch_entry(tx.conn(), &user, id).await.ok(),
        None => None,
    };
    match followup {
        // The continuation is untouched: merge it back into one entry.
        Some(next)
            if next.start_at == to
                && !next.locked
                && next.project_id == entry.project_id
                && next.tag_ids == entry.tag_ids
                && next.note == entry.note =>
        {
            soft_delete(tx, "time_entry", &next.id).await?;
            update_entry_row(
                tx,
                TimeEntry {
                    end_at: next.end_at,
                    ..entry
                },
            )
            .await?;
        }
        // Otherwise fill the gap with a copy of the interrupted entry.
        _ => {
            let filler = TimeEntry {
                id: crate::ids::new_id(),
                start_at: pause.start_at,
                end_at: Some(to),
                source: EntrySource::Manual,
                created_at: tx.now(),
                updated_at: tx.now(),
                ..entry
            };
            insert_entry(tx, &filler).await?;
        }
    }
    Ok(())
}
