//! Recent notes: remembers the notes of stopped entries, offers them in the
//! quick switch, and sends a summary when a day is over.

use extism_pdk::{config, plugin_fn, FnResult, Json};
use serde::{Deserialize, Serialize};
use time_addon_sdk::{host, Event, SuggestRequest, Suggestion};

const KEY: &str = "recent";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Recent {
    note: String,
    project_id: Option<String>,
    tag_ids: Vec<String>,
}

fn load() -> FnResult<Vec<Recent>> {
    Ok(host::kv_get(KEY)?
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default())
}

#[plugin_fn]
pub fn on_event(Json(event): Json<Event>) -> FnResult<()> {
    match event.kind.as_str() {
        "entry.stopped" => remember(event),
        "day.closed" => summarize(event),
        _ => Ok(()),
    }
}

fn remember(event: Event) -> FnResult<()> {
    let Some(entry) = event.entry() else { return Ok(()) };
    let note = entry.note.trim().to_string();
    if note.is_empty() {
        return Ok(());
    }
    let keep = config::get("keep")?.and_then(|v| v.parse().ok()).unwrap_or(10usize).clamp(1, 50);
    let mut list = load()?;
    list.retain(|r| r.note != note);
    list.insert(
        0,
        Recent {
            note,
            project_id: entry.project_id,
            tag_ids: entry.tag_ids,
        },
    );
    list.truncate(keep);
    host::kv_set(KEY, &serde_json::to_string(&list)?)
}

fn summarize(event: Event) -> FnResult<()> {
    if config::get("daily_summary")?.as_deref() == Some("false") {
        return Ok(());
    }
    let net = event.data.get("day").and_then(|d| d.get("netMs")).and_then(|v| v.as_i64()).unwrap_or(0);
    if net == 0 {
        return Ok(());
    }
    let minutes = net / 60_000;
    let date = event.date().unwrap_or_default();
    host::notify(&format!("{date}"), &format!("{}:{:02} h worked", minutes / 60, minutes % 60))
}

#[plugin_fn]
pub fn quickswitch_suggest(Json(req): Json<SuggestRequest>) -> FnResult<Json<Vec<Suggestion>>> {
    let query = req.query.trim().to_lowercase();
    let projects = host::list_projects()?;
    let out = load()?
        .into_iter()
        .filter(|r| query.is_empty() || r.note.to_lowercase().contains(&query))
        .take(5)
        .map(|r| {
            let project = r.project_id.as_deref().and_then(|id| projects.iter().find(|p| p.id == id));
            Suggestion {
                label: format!("Continue: {}", r.note),
                detail: project.map(|p| p.name.clone()),
                project_id: r.project_id,
                tag_ids: r.tag_ids,
                note: r.note,
            }
        })
        .collect();
    Ok(Json(out))
}
