//! SDK for Zeima addons (docs/addons.md).
//!
//! An addon is a WASM module plus an `addon.toml` manifest. The app calls
//! the module's exported functions for the hooks the manifest declares:
//!
//! | Hook | Export | Input | Output |
//! |---|---|---|---|
//! | `entry.*`, `day.closed` | `on_event` | [`Event`] | – |
//! | `schedule.tick` | `on_schedule_tick` | [`Tick`] | – |
//! | `report.export` | `export_report` | [`ExportRequest`] | [`ExportResult`] |
//! | `quickswitch.suggest` | `quickswitch_suggest` | [`SuggestRequest`] | `Vec<`[`Suggestion`]`>` |
//!
//! Inside those functions the addon can call the host API in [`host`]
//! (with the `guest` feature), limited to the manifest's permissions.
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
    pub group_name: Option<String>,
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
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Tick {
    /// Unix ms.
    pub now: i64,
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

/// Names of the host functions (module `extism:host/user`). Every input
/// and output is JSON.
pub mod host_fns {
    pub const LIST_ENTRIES: &str = "zeima_list_entries";
    pub const LIST_PROJECTS: &str = "zeima_list_projects";
    pub const LIST_TAGS: &str = "zeima_list_tags";
    pub const LIST_COST_CENTERS: &str = "zeima_list_cost_centers";
    pub const CREATE_ENTRY: &str = "zeima_create_entry";
    pub const NOTIFY: &str = "zeima_notify";
    pub const KV_GET: &str = "zeima_kv_get";
    pub const KV_SET: &str = "zeima_kv_set";
    pub const KV_KEYS: &str = "zeima_kv_keys";
}

/// The host API. Each call fails (and the hook with it) if the manifest
/// lacks the permission named in its docs.
#[cfg(feature = "guest")]
pub mod host {
    use extism_pdk::{host_fn, FnResult, Json};

    use super::*;

    #[host_fn]
    extern "ExtismHost" {
        fn zeima_list_entries(range: Json<Range>) -> Json<Vec<Entry>>;
        fn zeima_list_projects(unit: Json<()>) -> Json<Vec<Project>>;
        fn zeima_list_tags(unit: Json<()>) -> Json<Vec<Tag>>;
        fn zeima_list_cost_centers(unit: Json<()>) -> Json<Vec<CostCenter>>;
        fn zeima_create_entry(entry: Json<NewEntry>) -> Json<Entry>;
        fn zeima_notify(n: Json<Notification>) -> Json<()>;
        fn zeima_kv_get(key: Json<String>) -> Json<Option<String>>;
        fn zeima_kv_set(write: Json<KvWrite>) -> Json<()>;
        fn zeima_kv_keys(prefix: Json<String>) -> Json<Vec<String>>;
    }

    /// Entries overlapping `range` (`entries:read`).
    pub fn list_entries(range: Range) -> FnResult<Vec<Entry>> {
        Ok(unsafe { zeima_list_entries(Json(range)) }?.0)
    }

    /// All projects, archived ones included (`projects:read`).
    pub fn list_projects() -> FnResult<Vec<Project>> {
        Ok(unsafe { zeima_list_projects(Json(())) }?.0)
    }

    /// `tags:read`
    pub fn list_tags() -> FnResult<Vec<Tag>> {
        Ok(unsafe { zeima_list_tags(Json(())) }?.0)
    }

    /// `cost-centers:read`
    pub fn list_cost_centers() -> FnResult<Vec<CostCenter>> {
        Ok(unsafe { zeima_list_cost_centers(Json(())) }?.0)
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
}

/// The Extism PDK. Addons also add `extism-pdk` to their own
/// dependencies: its `#[plugin_fn]` macro refers to the crate by name.
#[cfg(feature = "guest")]
pub use extism_pdk as pdk;
