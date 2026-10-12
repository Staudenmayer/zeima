//! Custom CSV export: one row per entry, with the columns, delimiter and
//! decimal separator chosen in the addon's settings.

use std::collections::HashMap;

use extism_pdk::{config, plugin_fn, FnResult, Json};
use time_addon_sdk::{Entry, ExportRequest, ExportResult, Project};

#[plugin_fn]
pub fn export_report(Json(req): Json<ExportRequest>) -> FnResult<Json<ExportResult>> {
    let setting = |key: &str, default: &str| config::get(key).ok().flatten().unwrap_or_else(|| default.into());
    let columns: Vec<String> = setting("columns", "date,start,end,hours,project,note")
        .split(',')
        .map(|c| c.trim().to_lowercase())
        .filter(|c| !c.is_empty())
        .collect();
    let comma = match setting("decimal", "auto").as_str() {
        "comma" => true,
        "point" => false,
        _ => req.regional.number_format == "comma",
    };
    let delimiter = match setting("delimiter", "auto").as_str() {
        "tab" => "\t".to_string(),
        "auto" if comma => ";".into(),
        "auto" => ",".into(),
        other => other.to_string(),
    };
    let header = setting("header", "true") == "true";

    let projects: HashMap<&str, &Project> = req.projects.iter().map(|p| (p.id.as_str(), p)).collect();
    let tags: HashMap<&str, &str> = req.tags.iter().map(|t| (t.id.as_str(), t.name.as_str())).collect();
    let cost_centers: HashMap<&str, &str> = req.cost_centers.iter().map(|c| (c.id.as_str(), c.code.as_str())).collect();

    let mut out = String::from('\u{feff}');
    if header {
        out.push_str(&row(columns.iter().map(|c| c.to_string()), &delimiter));
    }
    for e in &req.entries {
        let cells = columns.iter().map(|c| cell(c, e, &req, &projects, &tags, &cost_centers, comma));
        out.push_str(&row(cells, &delimiter));
    }
    Ok(Json(ExportResult { content: out }))
}

fn row(cells: impl Iterator<Item = String>, delimiter: &str) -> String {
    let quoted: Vec<String> = cells
        .map(|c| {
            if c.contains(delimiter) || c.contains(['"', '\n', '\r']) {
                format!("\"{}\"", c.replace('"', "\"\""))
            } else {
                c
            }
        })
        .collect();
    quoted.join(delimiter) + "\r\n"
}

fn cell(
    column: &str,
    e: &Entry,
    req: &ExportRequest,
    projects: &HashMap<&str, &Project>,
    tags: &HashMap<&str, &str>,
    cost_centers: &HashMap<&str, &str>,
    comma: bool,
) -> String {
    let project = e.project_id.as_deref().and_then(|id| projects.get(id));
    let end = e.end_at.unwrap_or(e.start_at);
    let minutes = (end - e.start_at) / 60_000;
    let decimal = |v: f64| {
        let s = format!("{v:.2}");
        if comma {
            s.replace('.', ",")
        } else {
            s
        }
    };
    match column {
        // Local times come from the host as `2026-10-05T09:30`.
        "date" => e.local_start.as_deref().map(|s| date(&s[..10], &req.regional.date_format)).unwrap_or_default(),
        "start" => e.local_start.as_deref().map(|s| s[11..].to_string()).unwrap_or_default(),
        "end" => e.local_end.as_deref().map(|s| s[11..].to_string()).unwrap_or_default(),
        "hours" => decimal(minutes as f64 / 60.0),
        "minutes" => minutes.to_string(),
        "project" => project.map(|p| p.name.clone()).unwrap_or_default(),
        "alias" => project.and_then(|p| p.alias.clone()).unwrap_or_default(),
        "client" => project.and_then(|p| p.client.clone()).unwrap_or_default(),
        "tags" => e
            .tag_ids
            .iter()
            .filter_map(|id| tags.get(id.as_str()).copied())
            .collect::<Vec<_>>()
            .join(", "),
        "cost_center" => e
            .cost_center_id
            .as_deref()
            .and_then(|id| cost_centers.get(id).copied())
            .unwrap_or_default()
            .to_string(),
        "billable" => if e.billable { "1" } else { "0" }.into(),
        "note" => e.note.clone(),
        _ => String::new(),
    }
}

/// `2026-10-05` in the user's date format.
fn date(iso: &str, format: &str) -> String {
    let (y, m, d) = (&iso[..4], &iso[5..7], &iso[8..10]);
    match format {
        "mdy" => format!("{m}/{d}/{y}"),
        "ymd" => iso.to_string(),
        _ => format!("{d}.{m}.{y}"),
    }
}
