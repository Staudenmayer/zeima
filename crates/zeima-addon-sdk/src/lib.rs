//! SDK for Zeima addons (docs/addons.md).
//!
//! An addon is a WASM module plus an `addon.toml` manifest. The app calls
//! the module's exported functions for the hooks the manifest declares:
//!
//! | Hook | Export | Input | Output |
//! |---|---|---|---|
//! | `entry.*`, `day.closed`, `meeting.changed`, `absence.changed` | `on_event` | [`Event`] | – |
//! | `schedule.tick` | `on_schedule_tick` | [`Tick`] | – |
//! | `report.export` | `export_report` | [`ExportRequest`] | [`ExportResult`] |
//! | `quickswitch.suggest` | `quickswitch_suggest` | [`SuggestRequest`] | `Vec<`[`Suggestion`]`>` |
//! | `timeline.events` | `timeline_events` | [`TimelineRequest`] | `Vec<`[`TimelineEvent`]`>` |
//!
//! Inside those functions the addon can call the host API in [`host`]
//! (with the `guest` feature), limited to the manifest's permissions: entries,
//! projects, activities, tags, cost centers, meetings, absences and work days
//! can be read, entries created, desktop notifications shown, and the addon has
//! its own storage, keychain secrets and OAuth tokens.
//! Settings from the manifest's form arrive as Extism config strings:
//! `extism_pdk::config::get("key")`.
//!
//! All payloads are JSON with camelCase fields. Unknown fields are ignored,
//! so addons keep working when the app adds fields.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A time entry. Times are unix milliseconds (UTC).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Entry {
    pub id: String,
    /// What kind of work (e.g. Meeting). `None` on old entries.
    pub activity_id: Option<String>,
    pub project_id: Option<String>,
    pub start_at: i64,
    /// `None` while the timer runs.
    pub end_at: Option<i64>,
    /// IANA zone the entry was recorded in.
    pub tz: String,
    pub note: String,
    pub billable: bool,
    pub tag_ids: Vec<String>,
    pub cost_center_id: Option<String>,
    pub source: String,
    pub locked: bool,
    /// Start in the user's time zone, `2026-10-05T09:30` (exports and
    /// `list_entries`; addons have no time zone database).
    pub local_start: Option<String>,
    /// End in the user's time zone; `None` while running.
    pub local_end: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Project {
    pub id: String,
    pub name: String,
    /// Short code, e.g. `P-2024-017`.
    pub alias: Option<String>,
    pub color: String,
    pub parent_id: Option<String>,
    pub client: Option<String>,
    pub status: String,
    pub cost_center_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub color: String,
    /// `None` = global tag.
    pub project_id: Option<String>,
    /// Tag groups are no longer used by the app (the field stays so old data
    /// still reads); do not rely on it.
    pub group_name: Option<String>,
}

/// What kind of work an entry is (Development, Meeting, …).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Activity {
    pub id: String,
    pub name: String,
    pub color: String,
    /// Hidden from pickers; entries keep it.
    pub archived: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CostCenter {
    pub id: String,
    pub code: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub active: bool,
}

/// `[from, to)` in unix ms.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Range {
    pub from: i64,
    pub to: i64,
}

/// A hook event: `{ "type": "entry.started", "entry": {…} }`. `kind` is the
/// hook name; the rest stays raw JSON, read it with [`Event::entry`] or
/// serde.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten)]
    pub data: serde_json::Map<String, Value>,
}

impl Event {
    /// The entry of `entry.started/stopped/created/updated`.
    pub fn entry(&self) -> Option<Entry> {
        serde_json::from_value(self.data.get("entry")?.clone()).ok()
    }

    /// The id of `entry.deleted`.
    pub fn entry_id(&self) -> Option<String> {
        self.data.get("entryId")?.as_str().map(str::to_string)
    }

    /// The date (`YYYY-MM-DD`) of `day.closed`.
    pub fn date(&self) -> Option<String> {
        self.data.get("date")?.as_str().map(str::to_string)
    }

    /// The finished work day of `day.closed`.
    pub fn day(&self) -> Option<WorkDay> {
        serde_json::from_value(self.data.get("day")?.clone()).ok()
    }

    /// What happened, for `meeting.changed`: `created`, `updated`, `cancelled`,
    /// `answered` (the person's own answer) or `answerCleared`; for
    /// `absence.changed`: `created`, `updated`, `approved`, `rejected` or `deleted`.
    pub fn change(&self) -> Option<String> {
        self.data.get("change")?.as_str().map(str::to_string)
    }

    /// The meeting of `meeting.changed`, for every change (`meetingId`).
    pub fn meeting_id(&self) -> Option<String> {
        self.data.get("meetingId")?.as_str().map(str::to_string)
    }

    /// The meeting after the change (`created`, `updated`, `cancelled`); `None`
    /// for answers, which carry [`Event::meeting_id`] and [`Event::status`].
    pub fn meeting(&self) -> Option<Meeting> {
        serde_json::from_value(self.data.get("meeting")?.clone()).ok()
    }

    /// The answer of `meeting.changed` / `answered`: `accepted`, `tentative` or `declined`.
    pub fn status(&self) -> Option<String> {
        self.data.get("status")?.as_str().map(str::to_string)
    }

    /// The date (start, unix ms) of a one-date answer of `meeting.changed`; `None` for the whole series.
    pub fn occurrence_at(&self) -> Option<i64> {
        self.data.get("occurrenceAt")?.as_i64()
    }

    /// The id of `absence.changed`, for every change.
    pub fn absence_id(&self) -> Option<String> {
        self.data.get("absenceId")?.as_str().map(str::to_string)
    }

    /// The absence after the change; `None` for `deleted`.
    pub fn absence(&self) -> Option<Absence> {
        serde_json::from_value(self.data.get("absence")?.clone()).ok()
    }
}

/// `[from, to]` as local dates, both inclusive: `2026-10-05`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DateRange {
    pub from: String,
    pub to: String,
}

/// A meeting of an organization the person organizes or is asked to. The
/// times are those of the first date; one date of a repeating meeting is a
/// [`MeetingOccurrence`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Meeting {
    pub id: String,
    /// The organizer (account id).
    pub user_id: String,
    pub title: String,
    pub start_at: i64,
    pub end_at: i64,
    pub tz: String,
    /// Place; the room's name if a room is booked.
    pub location: Option<String>,
    pub room_id: Option<String>,
    pub online_url: Option<String>,
    /// What time spent in the meeting is tracked under.
    pub project_id: Option<String>,
    pub activity_id: Option<String>,
    pub tag_ids: Vec<String>,
    pub billable: bool,
    pub cost_center_id: Option<String>,
    pub note: String,
    /// `FREQ=WEEKLY;BYDAY=MO,WE;COUNT=10` style; `None` for a single meeting.
    pub rrule: Option<String>,
    /// Account ids of the people asked (not the organizer).
    pub attendee_ids: Vec<String>,
    pub cancelled: bool,
}

/// What a person answered: `accepted`, `tentative` or `declined`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MeetingResponse {
    /// Who answered (account id).
    pub user_id: String,
    pub status: String,
    /// Set when the answer is for one date of a repeating meeting (the start
    /// of that date, unix ms); `None` for an answer to the whole series.
    pub occurrence_at: Option<i64>,
}

/// One time a meeting happens, with the answers this device knows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MeetingOccurrence {
    pub meeting: Meeting,
    pub start_at: i64,
    pub end_at: i64,
    /// Each person's answer for this date: their own for the date if they gave
    /// one, else their answer to the series.
    pub responses: Vec<MeetingResponse>,
    /// The person's answer for this date: `accepted`, `tentative`, `declined`.
    pub my_status: Option<String>,
    /// The person's answer to the whole series, whatever they said for this date.
    pub series_status: Option<String>,
    /// Whether the person answered this date on its own.
    pub my_date_answer: bool,
    /// The person organizes the meeting (they do not answer it).
    pub is_organizer: bool,
}

/// An absence: vacation, sick leave and the like. Dates are local, inclusive.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Absence {
    pub id: String,
    pub user_id: String,
    /// The absence type's id.
    pub type_id: String,
    /// What the type is called (`Vacation`, `Sick leave`, or a custom name).
    pub type_name: String,
    /// `vacation`, `sick`, `comp_time`, `training`, `other` for the built-in types; `None` for custom ones.
    pub type_key: Option<String>,
    pub start_date: String,
    pub end_date: String,
    /// `am` or `pm` for half a day; `None` for whole days.
    pub half_day: Option<String>,
    /// `requested` until someone with the right decides (in an organization),
    /// then `approved` or `rejected`.
    pub status: String,
    pub note: String,
    /// Account id of whoever decided.
    pub decided_by: Option<String>,
    pub decision_note: String,
}

/// A warning on a work day: `{ "kind": "exceedsDailyMax", "maxMinutes": 600 }`,
/// `restTooShort` (`restMinutes`, `minMinutes`) or `breakTooShort`
/// (`requiredMinutes`, `takenMinutes`). Kept as JSON so new kinds do not
/// break addons.
pub type DayWarning = Value;

/// One local day of work time. All durations are milliseconds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkDay {
    pub date: String,
    /// Time covered by entries (overlaps count once).
    pub worked_ms: i64,
    pub break_taken_ms: i64,
    pub break_required_ms: i64,
    /// A missing break deducted from the worked time.
    pub break_deducted_ms: i64,
    /// Worked time after deductions; what counts.
    pub net_ms: i64,
    pub target_ms: i64,
    /// `net - target` (0 for trust-based schedules).
    pub delta_ms: i64,
    /// Manual balance corrections on this day.
    pub adjustment_ms: i64,
    pub first_start: Option<i64>,
    pub last_end: Option<i64>,
    /// Name of the holiday if the day is a public holiday (day off).
    pub holiday: Option<String>,
    pub absence_type_ids: Vec<String>,
    pub warnings: Vec<DayWarning>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Tick {
    /// Unix ms.
    pub now: i64,
    /// IANA zone of the user's days.
    pub timezone: String,
    /// Today in the user's zone: `2026-10-05`.
    pub local_date: String,
    /// The time now in the user's zone: `08:30` (24 hours).
    pub local_time: String,
}

/// The user's regional settings, with "automatic" already resolved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Regional {
    /// UI language: `de` or `en`.
    pub language: String,
    /// IANA zone for day boundaries.
    pub timezone: String,
    /// `dmy` (31.12.2026), `mdy` (12/31/2026) or `ymd` (2026-12-31).
    pub date_format: String,
    /// `h24` or `h12`.
    pub time_format: String,
    /// `point` or `comma`.
    pub number_format: String,
    /// `monday`, `sunday` or `saturday`.
    pub week_start: String,
}

/// One row of a grouped report; `key` is a project/tag/cost center id,
/// `"true"`/`"false"` (billable) or a period (`2026-10-05`, `2026-10`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReportRow {
    pub key: Option<String>,
    pub ms: i64,
    pub billable_ms: i64,
    pub entry_count: u32,
    pub children: Vec<ReportRow>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportRequest {
    /// The `export_formats` id the user picked.
    pub format_id: String,
    pub range: Range,
    /// Grouping levels, e.g. `["project", "day"]`.
    pub group_by: Vec<String>,
    pub total_ms: i64,
    pub billable_ms: i64,
    pub rows: Vec<ReportRow>,
    /// Matching entries, by start time.
    pub entries: Vec<Entry>,
    pub projects: Vec<Project>,
    pub tags: Vec<Tag>,
    pub cost_centers: Vec<CostCenter>,
    pub regional: Regional,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportResult {
    /// File contents (text; UTF-8).
    pub content: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SuggestRequest {
    /// What the user typed.
    pub query: String,
}

/// A quick-switch option. Choosing it starts the timer with these values;
/// without `activity_id` the user picks the activity first.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Suggestion {
    pub label: String,
    /// Second line, e.g. an issue title.
    pub detail: Option<String>,
    pub activity_id: Option<String>,
    pub project_id: Option<String>,
    pub tag_ids: Vec<String>,
    pub note: String,
}

/// Input of `timeline_events`: the days the timesheet shows.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TimelineRequest {
    pub range: Range,
    /// IANA zone the days are in.
    pub timezone: String,
}

/// An item in the timesheet calendar, e.g. a calendar event. "Track this
/// event" opens a new entry with its times and the entry fields below.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TimelineEvent {
    /// Unique within the addon (e.g. the calendar's event id).
    pub id: String,
    pub title: String,
    /// Unix ms.
    pub start_at: i64,
    pub end_at: i64,
    /// All-day items are listed in the day header, not on the grid.
    pub all_day: bool,
    /// Second line, e.g. the location.
    pub detail: Option<String>,
    /// `#rrggbb`; the app's neutral color if `None`.
    pub color: Option<String>,
    pub activity_id: Option<String>,
    pub project_id: Option<String>,
    pub tag_ids: Vec<String>,
    /// Note of the new entry; the title if `None`.
    pub note: Option<String>,
}

/// Input of [`host::create_entry`]. `cost_center_id: None` resolves it as
/// for manual entries (project, then the user's default).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NewEntry {
    /// Optional for addons; entries the user creates always have one.
    pub activity_id: Option<String>,
    pub project_id: Option<String>,
    pub start_at: i64,
    pub end_at: i64,
    pub note: String,
    /// `None` = the project's default.
    pub billable: Option<bool>,
    pub tag_ids: Vec<String>,
    pub cost_center_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Notification {
    pub title: String,
    pub body: String,
}

/// Value of a key-value write; `value: None` deletes the key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct KvWrite {
    pub key: String,
    pub value: Option<String>,
}

/// Value of a secret write; `value: None` deletes the secret.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SecretWrite {
    pub key: String,
    pub value: Option<String>,
}

/// Names of the host functions (module `extism:host/user`). Every input
/// and output is JSON.
pub mod host_fns {
    pub const LIST_ENTRIES: &str = "zeima_list_entries";
    pub const LIST_PROJECTS: &str = "zeima_list_projects";
    pub const LIST_ACTIVITIES: &str = "zeima_list_activities";
    pub const LIST_TAGS: &str = "zeima_list_tags";
    pub const LIST_COST_CENTERS: &str = "zeima_list_cost_centers";
    pub const LIST_MEETINGS: &str = "zeima_list_meetings";
    pub const LIST_ABSENCES: &str = "zeima_list_absences";
    pub const LIST_WORK_DAYS: &str = "zeima_list_work_days";
    pub const CREATE_ENTRY: &str = "zeima_create_entry";
    pub const NOTIFY: &str = "zeima_notify";
    pub const KV_GET: &str = "zeima_kv_get";
    pub const KV_SET: &str = "zeima_kv_set";
    pub const KV_KEYS: &str = "zeima_kv_keys";
    pub const SECRET_GET: &str = "zeima_secret_get";
    pub const SECRET_SET: &str = "zeima_secret_set";
    pub const OAUTH_TOKEN: &str = "zeima_oauth_token";
}

/// The host API. Each call fails (and the hook with it) if the manifest
/// lacks the permission named in its docs.
#[cfg(all(feature = "guest", target_arch = "wasm32"))]
pub mod host {
    use extism_pdk::{host_fn, FnResult, Json};

    use super::*;

    #[host_fn]
    extern "ExtismHost" {
        fn zeima_list_entries(range: Json<Range>) -> Json<Vec<Entry>>;
        fn zeima_list_projects(unit: Json<()>) -> Json<Vec<Project>>;
        fn zeima_list_activities(unit: Json<()>) -> Json<Vec<Activity>>;
        fn zeima_list_tags(unit: Json<()>) -> Json<Vec<Tag>>;
        fn zeima_list_cost_centers(unit: Json<()>) -> Json<Vec<CostCenter>>;
        fn zeima_list_meetings(range: Json<Range>) -> Json<Vec<MeetingOccurrence>>;
        fn zeima_list_absences(range: Json<DateRange>) -> Json<Vec<Absence>>;
        fn zeima_list_work_days(range: Json<DateRange>) -> Json<Vec<WorkDay>>;
        fn zeima_create_entry(entry: Json<NewEntry>) -> Json<Entry>;
        fn zeima_notify(n: Json<Notification>) -> Json<()>;
        fn zeima_kv_get(key: Json<String>) -> Json<Option<String>>;
        fn zeima_kv_set(write: Json<KvWrite>) -> Json<()>;
        fn zeima_kv_keys(prefix: Json<String>) -> Json<Vec<String>>;
        fn zeima_secret_get(key: Json<String>) -> Json<Option<String>>;
        fn zeima_secret_set(write: Json<SecretWrite>) -> Json<()>;
        fn zeima_oauth_token(provider: Json<String>) -> Json<Option<String>>;
    }

    /// Entries overlapping `range` (`entries:read`).
    pub fn list_entries(range: Range) -> FnResult<Vec<Entry>> {
        Ok(unsafe { zeima_list_entries(Json(range)) }?.0)
    }

    /// All projects, archived ones included (`projects:read`).
    pub fn list_projects() -> FnResult<Vec<Project>> {
        Ok(unsafe { zeima_list_projects(Json(())) }?.0)
    }

    /// All activities, archived ones included (`activities:read`).
    pub fn list_activities() -> FnResult<Vec<Activity>> {
        Ok(unsafe { zeima_list_activities(Json(())) }?.0)
    }

    /// `tags:read`
    pub fn list_tags() -> FnResult<Vec<Tag>> {
        Ok(unsafe { zeima_list_tags(Json(())) }?.0)
    }

    /// `cost-centers:read`
    pub fn list_cost_centers() -> FnResult<Vec<CostCenter>> {
        Ok(unsafe { zeima_list_cost_centers(Json(())) }?.0)
    }

    /// The meetings of the person (organized or asked to) that overlap `range`,
    /// one item per date, with the answers this device knows
    /// (`meetings:read`). Empty outside an organization's workspace.
    pub fn list_meetings(range: Range) -> FnResult<Vec<MeetingOccurrence>> {
        Ok(unsafe { zeima_list_meetings(Json(range)) }?.0)
    }

    /// The person's own absences overlapping the dates, with their approval
    /// status (`absences:read`).
    pub fn list_absences(range: DateRange) -> FnResult<Vec<Absence>> {
        Ok(unsafe { zeima_list_absences(Json(range)) }?.0)
    }

    /// Target, worked time, balance, holidays and warnings per local day
    /// (`work:read`). At most 366 days per call.
    pub fn list_work_days(range: DateRange) -> FnResult<Vec<WorkDay>> {
        Ok(unsafe { zeima_list_work_days(Json(range)) }?.0)
    }

    /// Creates a finished entry (`entries:write`).
    pub fn create_entry(entry: NewEntry) -> FnResult<Entry> {
        Ok(unsafe { zeima_create_entry(Json(entry)) }?.0)
    }

    /// Desktop notification (`notify`).
    pub fn notify(title: &str, body: &str) -> FnResult<()> {
        unsafe {
            zeima_notify(Json(Notification {
                title: title.into(),
                body: body.into(),
            }))
        }?;
        Ok(())
    }

    /// The addon's own storage; no permission needed.
    pub fn kv_get(key: &str) -> FnResult<Option<String>> {
        Ok(unsafe { zeima_kv_get(Json(key.to_string())) }?.0)
    }

    pub fn kv_set(key: &str, value: &str) -> FnResult<()> {
        unsafe {
            zeima_kv_set(Json(KvWrite {
                key: key.into(),
                value: Some(value.into()),
            }))
        }?;
        Ok(())
    }

    pub fn kv_delete(key: &str) -> FnResult<()> {
        unsafe {
            zeima_kv_set(Json(KvWrite {
                key: key.into(),
                value: None,
            }))
        }?;
        Ok(())
    }

    /// Keys starting with `prefix`, sorted.
    pub fn kv_keys(prefix: &str) -> FnResult<Vec<String>> {
        Ok(unsafe { zeima_kv_keys(Json(prefix.to_string())) }?.0)
    }

    /// A secret from the OS keychain (API keys, tokens); only this addon
    /// can read it. No permission needed.
    pub fn secret_get(key: &str) -> FnResult<Option<String>> {
        Ok(unsafe { zeima_secret_get(Json(key.to_string())) }?.0)
    }

    /// Stores a secret in the OS keychain (at most 8 KB).
    pub fn secret_set(key: &str, value: &str) -> FnResult<()> {
        unsafe {
            zeima_secret_set(Json(SecretWrite {
                key: key.into(),
                value: Some(value.into()),
            }))
        }?;
        Ok(())
    }

    pub fn secret_delete(key: &str) -> FnResult<()> {
        unsafe {
            zeima_secret_set(Json(SecretWrite {
                key: key.into(),
                value: None,
            }))
        }?;
        Ok(())
    }

    /// A valid access token for the manifest's `[[oauth]]` provider, for an
    /// `Authorization: Bearer …` header; refreshed by the app when needed.
    /// `None` while the user has not connected the account (or the
    /// provider revoked it).
    pub fn oauth_token(provider_id: &str) -> FnResult<Option<String>> {
        Ok(unsafe { zeima_oauth_token(Json(provider_id.to_string())) }?.0)
    }
}

/// The Extism PDK. Addons also add `extism-pdk` to their own
/// dependencies: its `#[plugin_fn]` macro refers to the crate by name.
#[cfg(all(feature = "guest", target_arch = "wasm32"))]
pub use extism_pdk as pdk;
