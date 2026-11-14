#![cfg(feature = "db")]

use std::sync::Arc;

use chrono::TimeZone;

use zeima_core::clock::ManualClock;
use zeima_core::import::{parse_import, DateOrder};
use zeima_core::summary::Range;
use zeima_core::Core;

const CSV: &str = "Client,Project,Description,Billable,Start date,Start time,End date,End time,Tags\n\
Acme,Website,Fix header,Yes,2026-10-05,09:00:00,2026-10-05,11:00:00,\"dev, urgent\"\n\
Acme,Website,Review,No,2026-10-05,13:00:00,2026-10-05,14:00:00,dev\n\
,,Admin,No,2026-10-06,10:00:00,2026-10-06,10:30:00,\n\
,,Broken,No,2026-10-06,11:00:00,2026-10-06,10:00:00,\n";

async fn core() -> Core {
    let core = Core::open_in_memory(Arc::new(ManualClock::new(1_791_187_200_000)))
        .await
        .unwrap();
    core.set_default_tz("Europe/Vienna").unwrap();
    core
}

#[tokio::test]
async fn imports_entries_with_their_projects_and_tags_and_does_nothing_twice() {
    let core = core().await;
    let parsed = parse_import(CSV, DateOrder::Ymd, "Europe/Vienna").unwrap();

    let preview = core.preview_import(&parsed).await.unwrap();
    assert_eq!((preview.new_entries, preview.duplicates), (3, 0));
    assert_eq!(preview.new_projects, vec!["Website"]);
    assert_eq!(preview.new_tags, vec!["dev", "urgent"]);
    assert_eq!(preview.skipped.len(), 1);
    // Previewing writes nothing.
    assert!(core.list_projects(true).await.unwrap().is_empty());

    let report = core.import_entries(&parsed).await.unwrap();
    assert_eq!(
        (
            report.created_entries,
            report.created_projects,
            report.created_tags
        ),
        (3, 1, 2)
    );
    let project = core.list_projects(true).await.unwrap().remove(0);
    assert_eq!(
        (project.name.as_str(), project.client.as_deref()),
        ("Website", Some("Acme"))
    );

    let entries = core
        .list_entries(Range {
            from: 1_790_000_000_000,
            to: 1_800_000_000_000,
        })
        .await
        .unwrap();
    assert_eq!(entries.len(), 3);
    let fix = entries.iter().find(|e| e.note == "Fix header").unwrap();
    assert!(fix.billable);
    assert_eq!(fix.tag_ids.len(), 2);
    assert_eq!(fix.project_id.as_deref(), Some(project.id.as_str()));
    // 09:00 in Vienna on 5 October 2026 is 07:00 UTC.
    assert_eq!(
        fix.start_at,
        chrono::Utc
            .with_ymd_and_hms(2026, 10, 5, 7, 0, 0)
            .unwrap()
            .timestamp_millis()
    );

    // The same file again changes nothing.
    let again = core.import_entries(&parsed).await.unwrap();
    assert_eq!(
        (
            again.created_entries,
            again.duplicates,
            again.created_projects,
            again.created_tags
        ),
        (0, 3, 0, 0)
    );
}
