//! Tells the person when something waits for their decision or was decided
//! (SPEC §5.9): derived from the rows a sync brings in, so every change is
//! reported once, on the device that receives it.

use tauri::AppHandle;
use tauri::Manager;
use tauri_plugin_notification::NotificationExt;
use zeima_core::RemoteRow;

use crate::state::AppState;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Notices {
    /// Timesheets of other people waiting for the person.
    pub timesheets_waiting: usize,
    pub absences_waiting: usize,
    /// The person's own, newly approved or rejected by someone else.
    pub approved: usize,
    pub rejected: usize,
}

impl Notices {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

fn text<'a>(row: &'a RemoteRow, field: &str) -> Option<&'a str> {
    row.fields.get(field)?.v.as_str()
}

/// What a batch of pulled rows means for `me`.
pub fn notices(rows: &[RemoteRow], me: &str) -> Notices {
    let mut n = Notices::default();
    for row in rows {
        if row.deleted.is_some() || !matches!(row.table.as_str(), "timesheet_submission" | "absence") {
            continue;
        }
        let (Some(owner), Some(status)) = (text(row, "userId"), text(row, "status")) else {
            continue;
        };
        let decided_by = text(row, "decidedBy");
        match (owner == me, status) {
            (false, "submitted") if row.table == "timesheet_submission" => n.timesheets_waiting += 1,
            (false, "requested") if row.table == "absence" => n.absences_waiting += 1,
            (true, "approved") if decided_by.is_some_and(|d| d != me) => n.approved += 1,
            (true, "rejected") if decided_by.is_some_and(|d| d != me) => n.rejected += 1,
            _ => {}
        }
    }
    n
}

/// Shows what `notices` found as desktop notifications.
pub fn show(app: &AppHandle, n: &Notices) {
    if n.is_empty() {
        return;
    }
    let de = app.state::<AppState>().settings().language() == "de";
    let mut lines: Vec<String> = Vec::new();
    if n.timesheets_waiting > 0 {
        lines.push(if de {
            format!(
                "{} Zeitnachweis(e) warten auf deine Freigabe",
                n.timesheets_waiting
            )
        } else {
            format!("{} timesheet(s) waiting for your approval", n.timesheets_waiting)
        });
    }
    if n.absences_waiting > 0 {
        lines.push(if de {
            format!(
                "{} Abwesenheitsantrag/-anträge warten auf dich",
                n.absences_waiting
            )
        } else {
            format!("{} absence request(s) waiting for you", n.absences_waiting)
        });
    }
    if n.approved > 0 {
        lines.push(if de {
            format!("{} Eintrag/Einträge freigegeben", n.approved)
        } else {
            format!("{} of yours approved", n.approved)
        });
    }
    if n.rejected > 0 {
        lines.push(if de {
            format!("{} Eintrag/Einträge abgelehnt", n.rejected)
        } else {
            format!("{} of yours rejected", n.rejected)
        });
    }
    let title = if de { "Freigaben" } else { "Approvals" };
    let _ = app
        .notification()
        .builder()
        .title(title)
        .body(lines.join("\n"))
        .show();
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;
    use zeima_core::sync::Versioned;

    use super::*;

    fn row(table: &str, owner: &str, status: &str, decided_by: Option<&str>) -> RemoteRow {
        let field = |v: serde_json::Value| Versioned { v, h: "0".into() };
        let mut fields = BTreeMap::new();
        fields.insert("userId".into(), field(json!(owner)));
        fields.insert("status".into(), field(json!(status)));
        if let Some(d) = decided_by {
            fields.insert("decidedBy".into(), field(json!(d)));
        }
        RemoteRow {
            workspace_id: "w".into(),
            table: table.into(),
            row_id: "r".into(),
            fields,
            deleted: None,
        }
    }

    #[test]
    fn reports_what_waits_and_what_was_decided_by_others() {
        let rows = vec![
            row("timesheet_submission", "ann", "submitted", None),
            row("timesheet_submission", "me", "submitted", None),
            row("absence", "ann", "requested", None),
            row("absence", "me", "approved", Some("boss")),
            row("timesheet_submission", "me", "rejected", Some("boss")),
            // My own decision on somebody else's, an absence of mine nobody decided, other tables.
            row("absence", "ann", "approved", Some("me")),
            row("absence", "me", "approved", None),
            row("time_entry", "ann", "submitted", None),
        ];
        assert_eq!(
            notices(&rows, "me"),
            Notices {
                timesheets_waiting: 1,
                absences_waiting: 1,
                approved: 1,
                rejected: 1
            }
        );
        assert!(notices(&[], "me").is_empty());
    }
}
