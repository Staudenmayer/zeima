#![cfg(feature = "db")]

//! Rows of one local workspace cannot be reached from another.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use zeima_core::clock::ManualClock;
use zeima_core::model::{BudgetPeriod, ProjectInput, ProjectStatus, TagInput};
use zeima_core::sync::Versioned;
use zeima_core::{Core, Error, RemoteRow};

const T0: i64 = 1_791_187_200_000;
const ORG_WS: &str = "0192f000-0000-7000-8000-00000000a001";
const ORG: &str = "0192f000-0000-7000-8000-00000000b001";
const ME: &str = "0192f000-0000-7000-8000-00000000c001";

async fn setup() -> (Core, Core) {
    let personal = common::with_activity(
        Core::open_in_memory(Arc::new(ManualClock::new(T0)))
            .await
            .unwrap(),
    )
    .await;
    personal
        .adopt_org_workspace(ORG_WS, ORG, "Acme", ME, "Ada")
        .await
        .unwrap();
    let org = personal.scoped(ORG_WS, ME);
    (personal, org)
}

fn tag(name: &str) -> TagInput {
    TagInput {
        name: name.into(),
        color: "#3b82f6".into(),
        project_id: None,
        group_name: None,
    }
}

fn project(name: &str) -> ProjectInput {
    ProjectInput {
        name: name.into(),
        alias: None,
        color: "#3b82f6".into(),
        parent_id: None,
        client: None,
        budget_minutes: None,
        budget_period: BudgetPeriod::Total,
        budget_warn_pct: 80,
        billable_default: false,
        status: ProjectStatus::Active,
        cost_center_id: None,
        visibility: Default::default(),
        manager_id: None,
        member_ids: Vec::new(),
    }
}

#[tokio::test]
async fn a_tag_of_another_workspace_is_not_found() {
    let (personal, org) = setup().await;
    let mine = personal.create_tag(tag("Mine")).await.unwrap();

    let err = org.update_tag(&mine.id, tag("Renamed")).await.unwrap_err();
    assert!(matches!(err, Error::NotFound { .. }), "{err:?}");
    let err = org.delete_tag(&mine.id).await.unwrap_err();
    assert!(matches!(err, Error::NotFound { .. }), "{err:?}");

    let tags = personal.list_tags().await.unwrap();
    assert!(tags.iter().any(|t| t.id == mine.id && t.name == "Mine"));
    assert!(org.list_tags().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_pulled_row_reusing_another_workspaces_id_leaves_it_unchanged() {
    let (personal, org) = setup().await;
    let mine = personal.create_project(project("Mine")).await.unwrap();

    let stamp = |v: serde_json::Value| Versioned {
        v,
        h: format!("{:015}-000000-server", T0 + 1000),
    };
    let fields: BTreeMap<String, Versioned> = [
        ("name", "Other".into()),
        ("color", "#ff0000".into()),
        ("status", "active".into()),
    ]
    .into_iter()
    .map(|(k, v): (&str, serde_json::Value)| (k.to_string(), stamp(v)))
    .collect();
    let row = RemoteRow {
        workspace_id: ORG_WS.into(),
        table: "project".into(),
        row_id: mine.id.clone(),
        fields,
        deleted: None,
    };
    org.sync_apply(&[row]).await.unwrap();

    let after = personal.get_project(&mine.id).await.unwrap();
    assert_eq!(after.name, "Mine");
    assert_eq!(after.workspace_id, mine.workspace_id);
    assert!(org.list_projects(false).await.unwrap().is_empty());
}

const ORG_WS_B: &str = "0192f000-0000-7000-8000-00000000a002";
const ORG_B: &str = "0192f000-0000-7000-8000-00000000b002";

/// Two organizations on one server: the account id is the same in both.
async fn two_orgs() -> (Core, Core, Core) {
    let (personal, a) = setup().await;
    personal
        .adopt_org_workspace(ORG_WS_B, ORG_B, "Globex", ME, "Ada")
        .await
        .unwrap();
    let b = personal.scoped(ORG_WS_B, ME);
    (personal, a, b)
}

#[tokio::test]
async fn a_running_entry_of_one_organization_is_out_of_reach_from_another() {
    use zeima_core::model::{ActivityInput, EntryInput, StartTimer, TimerState};
    use zeima_core::summary::Range;

    let (_personal, a, b) = two_orgs().await;
    let activity = a
        .create_activity(ActivityInput {
            name: "Work".into(),
            color: "#3b82f6".into(),
            archived: false,
        })
        .await
        .unwrap();
    let TimerState::Running { entry } = a
        .start_timer(StartTimer {
            activity_id: Some(activity.id.clone()),
            project_id: None,
            tag_ids: Vec::new(),
            note: "secret".into(),
            billable: None,
        })
        .await
        .unwrap()
    else {
        panic!("not running");
    };
    let before = b.sync_outbox(100).await.unwrap().changes.len();

    // Not visible.
    assert!(matches!(b.timer_state().await.unwrap(), TimerState::Idle));
    assert!(matches!(
        b.get_entry(&entry.id).await,
        Err(Error::NotFound { .. })
    ));
    let range = Range {
        from: 0,
        to: i64::MAX,
    };
    assert!(b.list_entries(range).await.unwrap().is_empty());
    // Not stoppable.
    assert!(matches!(b.stop_timer().await.unwrap(), TimerState::Idle));
    // Not updatable.
    let input = EntryInput {
        activity_id: Some(activity.id.clone()),
        project_id: None,
        start_at: entry.start_at,
        end_at: Some(entry.start_at + 60_000),
        tz: None,
        note: "changed".into(),
        billable: None,
        tag_ids: Vec::new(),
        cost_center_id: None,
    };
    let err = b.update_entry(&entry.id, input).await.unwrap_err();
    assert!(matches!(err, Error::NotFound { .. }), "{err:?}");

    // Nothing was logged for B, and A's entry is untouched.
    assert_eq!(b.sync_outbox(100).await.unwrap().changes.len(), before);
    let mine = a.get_entry(&entry.id).await.unwrap();
    assert_eq!(mine.note, "secret");
    assert!(mine.end_at.is_none());
}

#[tokio::test]
async fn a_synced_entry_cannot_link_a_tag_of_another_workspace() {
    let (_personal, a, b) = two_orgs().await;
    let foreign = b.create_tag(tag("Foreign")).await.unwrap();
    let own = a.create_tag(tag("Own")).await.unwrap();
    let activity = a
        .create_activity(zeima_core::model::ActivityInput {
            name: "Work".into(),
            color: "#3b82f6".into(),
            archived: false,
        })
        .await
        .unwrap();
    let stamp = |v: serde_json::Value| Versioned {
        v,
        h: format!("{:015}-000000-server", T0 + 1000),
    };
    let row_with = |entry_id: &str, tags: serde_json::Value| {
        let fields: BTreeMap<String, Versioned> = [
            ("userId", ME.into()),
            ("source", "manual".into()),
            ("activityId", activity.id.clone().into()),
            ("startAt", (T0).into()),
            ("endAt", (T0 + 60_000).into()),
            ("tz", "UTC".into()),
            ("note", "x".into()),
            ("tagIds", tags),
        ]
        .into_iter()
        .map(|(k, v): (&str, serde_json::Value)| (k.to_string(), stamp(v)))
        .collect();
        RemoteRow {
            workspace_id: ORG_WS.into(),
            table: "time_entry".into(),
            row_id: entry_id.into(),
            fields,
            deleted: None,
        }
    };
    // Control: the same row with a tag of its own workspace is applied.
    let good = "0192f000-0000-7000-8000-00000000e002";
    let report = a
        .sync_apply(&[row_with(good, serde_json::json!([own.id]))])
        .await
        .unwrap();
    assert_eq!(
        report.conflicts,
        0,
        "{report:?} {:?}",
        a.sync_conflicts()
            .await
            .unwrap()
            .iter()
            .map(|c| c.message.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(a.get_entry(good).await.unwrap().tag_ids, vec![own.id.clone()]);

    let bad = "0192f000-0000-7000-8000-00000000e001";
    let report = a
        .sync_apply(&[row_with(bad, serde_json::json!([foreign.id]))])
        .await
        .unwrap();
    assert_eq!(report.conflicts, 1, "{report:?}");
    assert!(matches!(a.get_entry(bad).await, Err(Error::NotFound { .. })));
}
