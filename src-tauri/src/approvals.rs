//! Tells the person when an absence request waits for their decision or was
//! decided (SPEC §5.9): derived from the rows a sync brings in, so every change is
//! reported once, on the device that receives it.

use tauri::AppHandle;
use tauri::Manager;
use tauri_plugin_notification::NotificationExt;
use zeima_core::RemoteRow;

use crate::state::AppState;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Notices {
    /// Absence requests of other people waiting for the person.
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
        if row.deleted.is_some() || row.table != "absence" {
            continue;
        }
        let (Some(owner), Some(status)) = (text(row, "userId"), text(row, "status")) else {
            continue;
        };
        let decided_by = text(row, "decidedBy");
        match (owner == me, status) {
            (false, "requested") => n.absences_waiting += 1,
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
            format!("{} Abwesenheit(en) freigegeben", n.approved)
        } else {
            format!("{} absence(s) of yours approved", n.approved)
        });
    }
    if n.rejected > 0 {
        lines.push(if de {
            format!("{} Abwesenheit(en) abgelehnt", n.rejected)
        } else {
            format!("{} absence(s) of yours rejected", n.rejected)
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
            row("absence", "ann", "requested", None),
            row("absence", "me", "approved", Some("boss")),
            row("absence", "me", "rejected", Some("boss")),
            // My own decision on somebody else's, an absence of mine nobody decided, other tables.
            row("absence", "ann", "approved", Some("me")),
            row("absence", "me", "approved", None),
            row("time_entry", "ann", "requested", None),
        ];
        assert_eq!(
            notices(&rows, "me"),
            Notices {
                absences_waiting: 1,
                approved: 1,
                rejected: 1
            }
        );
        assert!(notices(&[], "me").is_empty());
    }
}
