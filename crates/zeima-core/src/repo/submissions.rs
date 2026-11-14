//! Timesheets handed in for approval (SPEC §5.9). Shared workspaces only:
//! a person submits a period, someone with the right approves or rejects it.
//! The server enforces who may do what and locks the entries of an approved
//! period; this side keeps the records.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use super::{not_found, soft_delete};
use crate::db::Core;
use crate::{Error, Result};

crate::model::string_enum!(SubmissionStatus { Submitted => "submitted", Approved => "approved", Rejected => "rejected" });

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct TimesheetSubmission {
    pub id: String,
    /// The person the timesheet belongs to (their account id).
    pub user_id: String,
    pub period_start: NaiveDate,
    /// Inclusive.
    pub period_end: NaiveDate,
    /// Zone the period's days are in.
    pub tz: String,
    pub status: SubmissionStatus,
    pub note: String,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub submitted_at: i64,
    pub decided_by: Option<String>,
    #[cfg_attr(feature = "specta", specta(type = Option<specta_typescript::Number>))]
    pub decided_at: Option<i64>,
    pub decision_note: String,
}

fn from_row(r: &SqliteRow) -> Result<TimesheetSubmission> {
    Ok(TimesheetSubmission {
        id: r.try_get("id")?,
        user_id: r.try_get("user_id")?,
        period_start: r.try_get("period_start")?,
        period_end: r.try_get("period_end")?,
        tz: r.try_get("tz")?,
        status: r.try_get::<String, _>("status")?.parse()?,
        note: r.try_get("note")?,
        submitted_at: r.try_get("submitted_at")?,
        decided_by: r.try_get("decided_by")?,
        decided_at: r.try_get("decided_at")?,
        decision_note: r.try_get("decision_note")?,
    })
}

macro_rules! columns {
    () => {
        "id, user_id, period_start, period_end, tz, status, note, submitted_at, decided_by, decided_at, decision_note"
    };
}

impl Core {
    fn require_shared(&self) -> Result<()> {
        if self.ctx().shared {
            Ok(())
        } else {
            Err(Error::validation(
                "workspace",
                "timesheets are submitted in an organization's workspace",
            ))
        }
    }

    /// Submissions in this workspace that this device knows: the person's
    /// own, plus those of people whose timesheets they may approve.
    pub async fn list_submissions(&self) -> Result<Vec<TimesheetSubmission>> {
        let rows = sqlx::query(concat!(
            "SELECT ",
            columns!(),
            " FROM timesheet_submission
             WHERE workspace_id = ? AND deleted_at IS NULL ORDER BY period_start DESC, submitted_at DESC"
        ))
        .bind(&self.ctx().workspace_id)
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(from_row).collect()
    }

    /// Hands in the days `from..=to` (in the default time zone). A period
    /// that was rejected is submitted again; one that is waiting or approved
    /// cannot be submitted twice, and periods may not overlap.
    pub async fn submit_timesheet(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        note: &str,
    ) -> Result<TimesheetSubmission> {
        self.require_shared()?;
        if to < from {
            return Err(Error::validation("periodEnd", "must not be before the start"));
        }
        let mut tx = self.begin().await?;
        let user = tx.ctx().user_id.clone();
        let ws = tx.ctx().workspace_id.clone();
        let overlapping = sqlx::query(concat!(
            "SELECT ",
            columns!(),
            " FROM timesheet_submission
             WHERE workspace_id = ? AND user_id = ? AND deleted_at IS NULL AND period_start <= ? AND period_end >= ?"
        ))
        .bind(&ws)
        .bind(&user)
        .bind(to)
        .bind(from)
        .fetch_all(tx.conn())
        .await?;
        let mut replace: Option<TimesheetSubmission> = None;
        for row in &overlapping {
            let existing = from_row(row)?;
            if existing.status == SubmissionStatus::Rejected
                && existing.period_start == from
                && existing.period_end == to
            {
                replace = Some(existing);
            } else if existing.status != SubmissionStatus::Rejected {
                return Err(Error::Conflict(
                    "This period overlaps a timesheet that is waiting or approved.".into(),
                ));
            }
        }
        let s = tx.stamp();
        let tz = self.default_tz();
        let submission = TimesheetSubmission {
            id: replace.as_ref().map_or_else(crate::ids::new_id, |r| r.id.clone()),
            user_id: user.clone(),
            period_start: from,
            period_end: to,
            tz,
            status: SubmissionStatus::Submitted,
            note: note.trim().to_string(),
            submitted_at: s.now,
            decided_by: None,
            decided_at: None,
            decision_note: String::new(),
        };
        if replace.is_some() {
            sqlx::query(
                "UPDATE timesheet_submission SET tz = ?, status = 'submitted', note = ?, submitted_at = ?,
                    decided_by = NULL, decided_at = NULL, decision_note = '', updated_at = ?, hlc = ?, origin_device = ?
                 WHERE id = ?",
            )
            .bind(&submission.tz)
            .bind(&submission.note)
            .bind(s.now)
            .bind(s.now)
            .bind(&s.hlc)
            .bind(&s.device)
            .bind(&submission.id)
            .execute(tx.conn())
            .await?;
        } else {
            sqlx::query(
                "INSERT INTO timesheet_submission (id, workspace_id, user_id, period_start, period_end, tz, status, note,
                    submitted_at, created_at, updated_at, hlc, origin_device)
                 VALUES (?, ?, ?, ?, ?, ?, 'submitted', ?, ?, ?, ?, ?, ?)",
            )
            .bind(&submission.id)
            .bind(&ws)
            .bind(&user)
            .bind(from)
            .bind(to)
            .bind(&submission.tz)
            .bind(&submission.note)
            .bind(s.now)
            .bind(s.now)
            .bind(s.now)
            .bind(&s.hlc)
            .bind(&s.device)
            .execute(tx.conn())
            .await?;
        }
        tx.log_upsert("timesheet_submission", &submission.id, &s, &submission)
            .await?;
        tx.commit().await?;
        Ok(submission)
    }

    /// Approves or rejects a waiting timesheet. Not one's own.
    pub async fn decide_submission(
        &self,
        id: &str,
        approve: bool,
        note: &str,
    ) -> Result<TimesheetSubmission> {
        self.require_shared()?;
        let mut tx = self.begin().await?;
        let row = sqlx::query(concat!(
            "SELECT ",
            columns!(),
            " FROM timesheet_submission WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL"
        ))
        .bind(id)
        .bind(&tx.ctx().workspace_id)
        .fetch_optional(tx.conn())
        .await?
        .ok_or_else(|| not_found("timesheet", id))?;
        let mut sub = from_row(&row)?;
        if sub.user_id == tx.ctx().user_id {
            return Err(Error::validation("id", "you cannot decide on your own timesheet"));
        }
        if sub.status != SubmissionStatus::Submitted {
            return Err(Error::Conflict("This timesheet has been decided already.".into()));
        }
        let s = tx.stamp();
        sub.status = if approve {
            SubmissionStatus::Approved
        } else {
            SubmissionStatus::Rejected
        };
        sub.decided_by = Some(tx.ctx().user_id.clone());
        sub.decided_at = Some(s.now);
        sub.decision_note = note.trim().to_string();
        sqlx::query(
            "UPDATE timesheet_submission SET status = ?, decided_by = ?, decided_at = ?, decision_note = ?,
                updated_at = ?, hlc = ?, origin_device = ? WHERE id = ?",
        )
        .bind(sub.status.as_str())
        .bind(&sub.decided_by)
        .bind(sub.decided_at)
        .bind(&sub.decision_note)
        .bind(s.now)
        .bind(&s.hlc)
        .bind(&s.device)
        .bind(id)
        .execute(tx.conn())
        .await?;
        tx.log_upsert("timesheet_submission", id, &s, &sub).await?;
        tx.commit().await?;
        Ok(sub)
    }

    /// Reopens an approved timesheet: it goes back to the person as rejected
    /// (with the reason) and the server unlocks the period's entries.
    pub async fn reopen_submission(&self, id: &str, note: &str) -> Result<TimesheetSubmission> {
        self.require_shared()?;
        let mut tx = self.begin().await?;
        let row = sqlx::query(concat!(
            "SELECT ",
            columns!(),
            " FROM timesheet_submission WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL"
        ))
        .bind(id)
        .bind(&tx.ctx().workspace_id)
        .fetch_optional(tx.conn())
        .await?
        .ok_or_else(|| not_found("timesheet", id))?;
        let mut sub = from_row(&row)?;
        if sub.user_id == tx.ctx().user_id {
            return Err(Error::validation("id", "you cannot reopen your own timesheet"));
        }
        if sub.status != SubmissionStatus::Approved {
            return Err(Error::Conflict(
                "Only an approved timesheet can be reopened.".into(),
            ));
        }
        let s = tx.stamp();
        sub.status = SubmissionStatus::Rejected;
        sub.decided_by = Some(tx.ctx().user_id.clone());
        sub.decided_at = Some(s.now);
        sub.decision_note = note.trim().to_string();
        sqlx::query(
            "UPDATE timesheet_submission SET status = 'rejected', decided_by = ?, decided_at = ?, decision_note = ?,
                updated_at = ?, hlc = ?, origin_device = ? WHERE id = ?",
        )
        .bind(&sub.decided_by)
        .bind(sub.decided_at)
        .bind(&sub.decision_note)
        .bind(s.now)
        .bind(&s.hlc)
        .bind(&s.device)
        .bind(id)
        .execute(tx.conn())
        .await?;
        tx.log_upsert("timesheet_submission", id, &s, &sub).await?;
        tx.commit().await?;
        Ok(sub)
    }

    /// Takes back a timesheet that is still waiting.
    pub async fn withdraw_submission(&self, id: &str) -> Result<()> {
        self.require_shared()?;
        let mut tx = self.begin().await?;
        let row = sqlx::query(concat!(
            "SELECT ",
            columns!(),
            " FROM timesheet_submission WHERE id = ? AND workspace_id = ? AND user_id = ? AND deleted_at IS NULL"
        ))
        .bind(id)
        .bind(&tx.ctx().workspace_id)
        .bind(&tx.ctx().user_id)
        .fetch_optional(tx.conn())
        .await?
        .ok_or_else(|| not_found("timesheet", id))?;
        if from_row(&row)?.status != SubmissionStatus::Submitted {
            return Err(Error::Conflict(
                "Only a timesheet that is waiting can be taken back.".into(),
            ));
        }
        soft_delete(&mut tx, "timesheet_submission", id).await?;
        tx.commit().await
    }
}
