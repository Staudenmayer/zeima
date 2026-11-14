//! Meetings of a shared workspace (SPEC §5.12): the organizer creates them,
//! the people asked answer, and everyone sees them in the timesheet. The
//! server sends a meeting only to those asked.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use crate::db::Core;
use crate::meeting::{is_valid_rule, occurrences};
use crate::{Error, Result};

crate::model::string_enum!(RsvpStatus { Accepted => "accepted", Declined => "declined", Tentative => "tentative" });

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Meeting {
    pub id: String,
    /// The organizer (account id).
    pub user_id: String,
    pub title: String,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub start_at: i64,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub end_at: i64,
    pub tz: String,
    pub location: Option<String>,
    /// The room of the organization it is held in (then `location` is its name).
    pub room_id: Option<String>,
    pub online_url: Option<String>,
    pub project_id: Option<String>,
    /// The activity time spent in it is tracked under.
    pub activity_id: Option<String>,
    /// Tags, billable and cost center for the time tracked in it (none: automatic).
    pub tag_ids: Vec<String>,
    pub billable: bool,
    pub cost_center_id: Option<String>,
    pub note: String,
    /// `FREQ=WEEKLY;BYDAY=MO;COUNT=10` style; see [`crate::meeting`].
    pub rrule: Option<String>,
    /// Account ids of the people asked (not the organizer).
    pub attendee_ids: Vec<String>,
    pub cancelled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct MeetingInput {
    pub title: String,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub start_at: i64,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub end_at: i64,
    pub tz: Option<String>,
    pub location: Option<String>,
    /// A room of the organization; its name becomes the location.
    #[serde(default)]
    pub room_id: Option<String>,
    pub online_url: Option<String>,
    pub project_id: Option<String>,
    /// The activity time spent in it is tracked under.
    pub activity_id: Option<String>,
    /// Tags, billable and cost center for the time tracked in it (none: automatic).
    pub tag_ids: Vec<String>,
    pub billable: bool,
    pub cost_center_id: Option<String>,
    pub note: String,
    pub rrule: Option<String>,
    pub attendee_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct MeetingResponse {
    pub id: String,
    /// Who answered (account id).
    pub user_id: String,
    pub meeting_id: String,
    pub status: RsvpStatus,
}

/// One time a meeting happens, with the answers this device knows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct MeetingOccurrence {
    pub meeting: Meeting,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub start_at: i64,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub end_at: i64,
    pub responses: Vec<MeetingResponse>,
    /// The person's own answer, if they gave one.
    pub my_status: Option<RsvpStatus>,
}

/// How far ahead a room booking is checked (repeating meetings).
const ROOM_HORIZON_MS: i64 = 2 * 365 * 24 * 3_600_000;

macro_rules! columns {
    () => {
        "id, user_id, title, start_at, end_at, tz, location, room_id, online_url, project_id, activity_id, tag_ids_json, billable, cost_center_id, note, rrule, attendee_ids_json, cancelled"
    };
}

fn from_row(r: &SqliteRow) -> Result<Meeting> {
    Ok(Meeting {
        id: r.try_get("id")?,
        user_id: r.try_get("user_id")?,
        title: r.try_get("title")?,
        start_at: r.try_get("start_at")?,
        end_at: r.try_get("end_at")?,
        tz: r.try_get("tz")?,
        location: r.try_get("location")?,
        room_id: r.try_get("room_id")?,
        online_url: r.try_get("online_url")?,
        project_id: r.try_get("project_id")?,
        activity_id: r.try_get("activity_id")?,
        tag_ids: serde_json::from_str(&r.try_get::<String, _>("tag_ids_json")?).unwrap_or_default(),
        billable: r.try_get("billable")?,
        cost_center_id: r.try_get("cost_center_id")?,
        note: r.try_get("note")?,
        rrule: r.try_get("rrule")?,
        attendee_ids: serde_json::from_str(&r.try_get::<String, _>("attendee_ids_json")?).unwrap_or_default(),
        cancelled: r.try_get("cancelled")?,
    })
}

fn response_id(workspace_id: &str, meeting_id: &str, user_id: &str) -> String {
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!("meeting_response|{workspace_id}|{meeting_id}|{user_id}").as_bytes(),
    )
    .to_string()
}

fn trimmed(text: &Option<String>) -> Option<String> {
    text.as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(String::from)
}

impl MeetingInput {
    fn normalized(mut self, me: &str, default_tz: &str) -> Result<Self> {
        self.title = self.title.trim().to_string();
        if self.title.is_empty() || self.title.chars().count() > 200 {
            return Err(Error::validation("title", "must be 1 to 200 characters"));
        }
        if self.end_at <= self.start_at {
            return Err(Error::validation("endAt", "must be after the start"));
        }
        let tz = self.tz.take().unwrap_or_else(|| default_tz.to_string());
        crate::model::validate_tz(&tz)?;
        self.tz = Some(tz);
        self.location = trimmed(&self.location);
        self.room_id = trimmed(&self.room_id);
        self.online_url = trimmed(&self.online_url);
        if let Some(url) = &self.online_url {
            if !url.starts_with("https://") && !url.starts_with("http://") {
                return Err(Error::validation(
                    "onlineUrl",
                    "must be a web address (https://…)",
                ));
            }
        }
        self.project_id = trimmed(&self.project_id);
        self.activity_id = trimmed(&self.activity_id);
        self.cost_center_id = trimmed(&self.cost_center_id);
        self.tag_ids.retain(|t| !t.trim().is_empty());
        self.tag_ids.sort();
        self.tag_ids.dedup();
        self.rrule = trimmed(&self.rrule);
        if self.rrule.as_deref().is_some_and(|r| !is_valid_rule(r)) {
            return Err(Error::validation("rrule", "this repeat rule is not supported"));
        }
        self.attendee_ids.retain(|a| a != me && !a.trim().is_empty());
        self.attendee_ids.sort();
        self.attendee_ids.dedup();
        if self.attendee_ids.len() > 200 {
            return Err(Error::validation("attendeeIds", "at most 200 people"));
        }
        Ok(self)
    }
}

impl Core {
    fn require_shared_meetings(&self) -> Result<()> {
        if self.ctx().shared {
            Ok(())
        } else {
            Err(Error::validation(
                "workspace",
                "meetings are scheduled in an organization's workspace",
            ))
        }
    }

    async fn get_meeting(&self, id: &str) -> Result<Meeting> {
        let row = sqlx::query(concat!(
            "SELECT ",
            columns!(),
            " FROM meeting WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL"
        ))
        .bind(id)
        .bind(&self.ctx().workspace_id)
        .fetch_optional(self.pool())
        .await?
        .ok_or_else(|| super::not_found("meeting", id))?;
        from_row(&row)
    }

    pub async fn create_meeting(&self, input: MeetingInput) -> Result<Meeting> {
        self.require_shared_meetings()?;
        let input = input.normalized(&self.ctx().user_id, &self.default_tz())?;
        self.write_meeting(None, input).await
    }

    /// Changes a meeting the person organizes.
    pub async fn update_meeting(&self, id: &str, input: MeetingInput) -> Result<Meeting> {
        self.require_shared_meetings()?;
        let existing = self.get_meeting(id).await?;
        if existing.user_id != self.ctx().user_id {
            return Err(Error::validation("id", "only the organizer can change a meeting"));
        }
        let input = input.normalized(&self.ctx().user_id, &self.default_tz())?;
        self.write_meeting(Some(existing), input).await
    }

    async fn write_meeting(&self, existing: Option<Meeting>, mut input: MeetingInput) -> Result<Meeting> {
        // A room must be one of the organization's; its name is the place shown.
        if let Some(room) = input.room_id.clone() {
            let name: Option<String> = sqlx::query_scalar(
                "SELECT name FROM room WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL",
            )
            .bind(&room)
            .bind(&self.ctx().workspace_id)
            .fetch_optional(self.pool())
            .await?;
            input.location = Some(name.ok_or_else(|| Error::validation("roomId", "unknown room"))?);
            self.check_room_free(existing.as_ref().map(|m| m.id.as_str()), &room, &input)
                .await?;
        }
        let mut tx = self.begin().await?;
        let s = tx.stamp();
        let meeting = Meeting {
            id: existing
                .as_ref()
                .map_or_else(crate::ids::new_id, |m| m.id.clone()),
            user_id: tx.ctx().user_id.clone(),
            title: input.title,
            start_at: input.start_at,
            end_at: input.end_at,
            tz: input.tz.unwrap_or_default(),
            location: input.location,
            room_id: input.room_id,
            online_url: input.online_url,
            project_id: input.project_id,
            activity_id: input.activity_id,
            tag_ids: input.tag_ids,
            billable: input.billable,
            cost_center_id: input.cost_center_id,
            note: input.note,
            rrule: input.rrule,
            attendee_ids: input.attendee_ids,
            cancelled: false,
        };
        sqlx::query(
            "INSERT INTO meeting (id, workspace_id, user_id, title, start_at, end_at, tz, location, room_id, online_url, project_id,
                activity_id, tag_ids_json, billable, cost_center_id, note, rrule, attendee_ids_json, cancelled, created_at, updated_at, hlc, origin_device)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET title = excluded.title, start_at = excluded.start_at, end_at = excluded.end_at,
                tz = excluded.tz, location = excluded.location, room_id = excluded.room_id, online_url = excluded.online_url,
                project_id = excluded.project_id, activity_id = excluded.activity_id, tag_ids_json = excluded.tag_ids_json, billable = excluded.billable,
                cost_center_id = excluded.cost_center_id, note = excluded.note, rrule = excluded.rrule,
                attendee_ids_json = excluded.attendee_ids_json, cancelled = 0, updated_at = excluded.updated_at,
                hlc = excluded.hlc, origin_device = excluded.origin_device",
        )
        .bind(&meeting.id)
        .bind(tx.ctx().workspace_id.clone())
        .bind(&meeting.user_id)
        .bind(&meeting.title)
        .bind(meeting.start_at)
        .bind(meeting.end_at)
        .bind(&meeting.tz)
        .bind(&meeting.location)
        .bind(&meeting.room_id)
        .bind(&meeting.online_url)
        .bind(&meeting.project_id)
        .bind(&meeting.activity_id)
        .bind(serde_json::to_string(&meeting.tag_ids)?)
        .bind(meeting.billable)
        .bind(&meeting.cost_center_id)
        .bind(&meeting.note)
        .bind(&meeting.rrule)
        .bind(serde_json::to_string(&meeting.attendee_ids)?)
        .bind(s.now)
        .bind(s.now)
        .bind(&s.hlc)
        .bind(&s.device)
        .execute(tx.conn())
        .await?;
        tx.log_upsert("meeting", &meeting.id, &s, &meeting).await?;
        tx.commit().await?;
        Ok(meeting)
    }

    /// Refuses a booking that overlaps another meeting in the room, repeats
    /// included. This device only knows the meetings it was asked to; the
    /// server checks against all of them.
    async fn check_room_free(&self, own_id: Option<&str>, room: &str, input: &MeetingInput) -> Result<()> {
        let tz = input.tz.clone().unwrap_or_default();
        let horizon = input.start_at + ROOM_HORIZON_MS;
        let mine = occurrences(
            input.start_at,
            input.end_at,
            &tz,
            input.rrule.as_deref(),
            input.start_at,
            horizon,
        );
        let others: Vec<Meeting> = sqlx::query(concat!(
            "SELECT ",
            columns!(),
            " FROM meeting WHERE workspace_id = ? AND room_id = ? AND cancelled = 0 AND deleted_at IS NULL"
        ))
        .bind(&self.ctx().workspace_id)
        .bind(room)
        .fetch_all(self.pool())
        .await?
        .iter()
        .map(from_row)
        .collect::<Result<_>>()?;
        for m in others.iter().filter(|m| Some(m.id.as_str()) != own_id) {
            let theirs = occurrences(
                m.start_at,
                m.end_at,
                &m.tz,
                m.rrule.as_deref(),
                input.start_at,
                horizon,
            );
            if mine
                .iter()
                .any(|(a, b)| theirs.iter().any(|(c, d)| a < d && c < b))
            {
                return Err(Error::Conflict("the room is taken at that time".into()));
            }
        }
        Ok(())
    }

    /// Calls a meeting off; it stays in the list, marked cancelled.
    pub async fn cancel_meeting(&self, id: &str) -> Result<Meeting> {
        self.require_shared_meetings()?;
        let mut meeting = self.get_meeting(id).await?;
        if meeting.user_id != self.ctx().user_id {
            return Err(Error::validation("id", "only the organizer can cancel a meeting"));
        }
        let mut tx = self.begin().await?;
        let s = tx.stamp();
        meeting.cancelled = true;
        sqlx::query(
            "UPDATE meeting SET cancelled = 1, updated_at = ?, hlc = ?, origin_device = ? WHERE id = ?",
        )
        .bind(s.now)
        .bind(&s.hlc)
        .bind(&s.device)
        .bind(id)
        .execute(tx.conn())
        .await?;
        tx.log_upsert("meeting", id, &s, &meeting).await?;
        tx.commit().await?;
        Ok(meeting)
    }

    /// The person's answer to a meeting they are asked to.
    pub async fn respond_to_meeting(&self, meeting_id: &str, status: RsvpStatus) -> Result<MeetingResponse> {
        self.require_shared_meetings()?;
        let meeting = self.get_meeting(meeting_id).await?;
        let me = self.ctx().user_id.clone();
        if meeting.user_id == me || !meeting.attendee_ids.contains(&me) {
            return Err(Error::validation(
                "meetingId",
                "you are not asked to this meeting",
            ));
        }
        let mut tx = self.begin().await?;
        let ws = tx.ctx().workspace_id.clone();
        // An answer that already exists (made on another device, or by older data with
        // another id) is changed in place: there is one live answer per person and meeting.
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id FROM meeting_response
             WHERE workspace_id = ? AND meeting_id = ? AND user_id = ? AND deleted_at IS NULL",
        )
        .bind(&ws)
        .bind(meeting_id)
        .bind(&me)
        .fetch_optional(tx.conn())
        .await?;
        let id = existing.unwrap_or_else(|| response_id(&ws, meeting_id, &me));
        let s = tx.stamp();
        sqlx::query(
            "INSERT INTO meeting_response (id, workspace_id, user_id, meeting_id, status, created_at, updated_at, hlc, origin_device)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET status = excluded.status, deleted_at = NULL,
                updated_at = excluded.updated_at, hlc = excluded.hlc, origin_device = excluded.origin_device",
        )
        .bind(&id)
        .bind(&ws)
        .bind(&me)
        .bind(meeting_id)
        .bind(status.as_str())
        .bind(s.now)
        .bind(s.now)
        .bind(&s.hlc)
        .bind(&s.device)
        .execute(tx.conn())
        .await?;
        let response = MeetingResponse {
            id: id.clone(),
            user_id: me,
            meeting_id: meeting_id.to_string(),
            status,
        };
        tx.log_upsert("meeting_response", &id, &s, &response).await?;
        tx.commit().await?;
        Ok(response)
    }

    /// The times meetings happen within `from..=to` (the person's days in the default zone),
    /// with the answers this device has.
    pub async fn list_meetings(&self, from: NaiveDate, to: NaiveDate) -> Result<Vec<MeetingOccurrence>> {
        let tz: chrono_tz::Tz = self.default_tz().parse().unwrap_or(chrono_tz::UTC);
        let day_ms = |d: NaiveDate| {
            use chrono::TimeZone;
            tz.from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap_or_default())
                .earliest()
                .map_or(0, |t| t.timestamp_millis())
        };
        self.meetings_between(day_ms(from), day_ms(to + chrono::Duration::days(1)))
            .await
    }

    /// The times meetings happen within `[from_ms, to_ms)`, with the answers this device has.
    pub async fn meetings_between(&self, from_ms: i64, to_ms: i64) -> Result<Vec<MeetingOccurrence>> {
        let meetings: Vec<Meeting> = sqlx::query(concat!(
            "SELECT ",
            columns!(),
            " FROM meeting WHERE workspace_id = ? AND deleted_at IS NULL"
        ))
        .bind(&self.ctx().workspace_id)
        .fetch_all(self.pool())
        .await?
        .iter()
        .map(from_row)
        .collect::<Result<_>>()?;
        let responses: Vec<MeetingResponse> = sqlx::query("SELECT id, user_id, meeting_id, status FROM meeting_response WHERE workspace_id = ? AND deleted_at IS NULL")
            .bind(&self.ctx().workspace_id)
            .fetch_all(self.pool())
            .await?
            .iter()
            .map(|r| {
                Ok(MeetingResponse {
                    id: r.try_get("id")?,
                    user_id: r.try_get("user_id")?,
                    meeting_id: r.try_get("meeting_id")?,
                    status: r.try_get::<String, _>("status")?.parse()?,
                })
            })
            .collect::<Result<_>>()?;
        let me = &self.ctx().user_id;
        let mut out = Vec::new();
        for m in meetings {
            let asked = m.user_id == *me || m.attendee_ids.contains(me);
            if !asked {
                continue;
            }
            let mine: Vec<MeetingResponse> = responses
                .iter()
                .filter(|r| r.meeting_id == m.id)
                .cloned()
                .collect();
            let my_status = mine.iter().find(|r| r.user_id == *me).map(|r| r.status);
            for (start_at, end_at) in
                occurrences(m.start_at, m.end_at, &m.tz, m.rrule.as_deref(), from_ms, to_ms)
            {
                out.push(MeetingOccurrence {
                    meeting: m.clone(),
                    start_at,
                    end_at,
                    responses: mine.clone(),
                    my_status,
                });
            }
        }
        out.sort_by_key(|o| (o.start_at, o.meeting.id.clone()));
        Ok(out)
    }

    /// The meeting as an iCalendar file, for any calendar program.
    pub async fn meeting_ics(&self, id: &str) -> Result<String> {
        let m = self.get_meeting(id).await?;
        let stamp = |ms: i64| {
            chrono::DateTime::from_timestamp_millis(ms)
                .map(|d| d.format("%Y%m%dT%H%M%SZ").to_string())
                .unwrap_or_default()
        };
        let esc = |t: &str| {
            t.replace('\\', "\\\\")
                .replace(';', "\\;")
                .replace(',', "\\,")
                .replace('\n', "\\n")
        };
        let mut lines = vec![
            "BEGIN:VCALENDAR".to_string(),
            "VERSION:2.0".into(),
            "PRODID:-//Zeima//Meeting//EN".into(),
            "BEGIN:VEVENT".into(),
            format!("UID:{}@zeima", m.id),
            format!("DTSTAMP:{}", stamp(self.now_ms())),
            format!("DTSTART:{}", stamp(m.start_at)),
            format!("DTEND:{}", stamp(m.end_at)),
            format!("SUMMARY:{}", esc(&m.title)),
        ];
        if let Some(rule) = &m.rrule {
            lines.push(format!("RRULE:{rule}"));
        }
        if let Some(l) = &m.location {
            lines.push(format!("LOCATION:{}", esc(l)));
        }
        if let Some(u) = &m.online_url {
            lines.push(format!("URL:{u}"));
        }
        if !m.note.is_empty() {
            lines.push(format!("DESCRIPTION:{}", esc(&m.note)));
        }
        if m.cancelled {
            lines.push("STATUS:CANCELLED".into());
        }
        lines.extend(["END:VEVENT".into(), "END:VCALENDAR".into()]);
        Ok(lines.join("\r\n") + "\r\n")
    }
}
