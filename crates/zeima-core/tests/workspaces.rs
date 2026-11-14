#![cfg(feature = "db")]

mod common;

use std::sync::Arc;

use zeima_core::clock::ManualClock;
use zeima_core::model::{ActivityInput, BudgetPeriod, EntryInput, ProjectInput, ProjectStatus};
use zeima_core::summary::Range;
use zeima_core::Core;

const T0: i64 = 1_791_187_200_000;
const HOUR: i64 = 3_600_000;
const ORG_WS: &str = "0192f000-0000-7000-8000-00000000a001";
const ORG: &str = "0192f000-0000-7000-8000-00000000b001";
const ME: &str = "0192f000-0000-7000-8000-00000000c001";

fn project(name: &str) -> ProjectInput {
    ProjectInput {
        name: name.into(),
        alias: Some("P".into()),
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

#[tokio::test]
async fn lists_the_personal_workspace_first_and_adopting_is_idempotent() {
    let (personal, _) = setup().await;
    personal
        .adopt_org_workspace(ORG_WS, ORG, "Acme Inc", ME, "Ada")
        .await
        .unwrap();
    let all = personal.workspaces().await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].kind, "personal");
    assert_eq!(
        (
            all[1].id.as_str(),
            all[1].kind.as_str(),
            all[1].org_id.as_deref(),
            all[1].name.as_str()
        ),
        (ORG_WS, "org", Some(ORG), "Acme Inc")
    );
}

#[tokio::test]
async fn each_workspace_sees_only_its_own_data() {
    let (personal, org) = setup().await;
    // Same alias in both: the unique index is per workspace.
    personal.create_project(project("Mine")).await.unwrap();
    org.create_project(project("Shared")).await.unwrap();
    let names = |c: &Core| {
        let c = c.scoped(&c.ctx().workspace_id, &c.ctx().user_id);
        async move {
            c.list_projects(false)
                .await
                .unwrap()
                .into_iter()
                .map(|p| p.name)
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(names(&personal).await, vec!["Mine"]);
    assert_eq!(names(&org).await, vec!["Shared"]);

    // Activities and entries are separate too.
    let activity = org
        .create_activity(ActivityInput {
            name: "Work".into(),
            color: "#111111".into(),
            archived: false,
        })
        .await
        .unwrap();
    org.create_entry(EntryInput {
        activity_id: Some(activity.id),
        project_id: None,
        start_at: T0,
        end_at: Some(T0 + HOUR),
        tz: Some("UTC".into()),
        note: "org work".into(),
        billable: None,
        tag_ids: vec![],
        cost_center_id: None,
    })
    .await
    .unwrap();
    let range = Range {
        from: T0 - HOUR,
        to: T0 + 2 * HOUR,
    };
    assert_eq!(org.list_entries(range).await.unwrap().len(), 1);
    assert!(personal.list_entries(range).await.unwrap().is_empty());
}

#[tokio::test]
async fn scoped_handles_share_settings_and_the_event_stream() {
    let (personal, org) = setup().await;
    org.set_default_tz("Europe/Vienna").unwrap();
    assert_eq!(personal.default_tz(), "Europe/Vienna");
    // Events published through one handle reach subscribers of the other.
    let mut events = personal.subscribe();
    org.create_activity(ActivityInput {
        name: "Work".into(),
        color: "#111111".into(),
        archived: false,
    })
    .await
    .unwrap();
    org.create_project(project("Shared")).await.unwrap();
    let activity = org.list_activities(false).await.unwrap().remove(0);
    org.create_entry(EntryInput {
        activity_id: Some(activity.id),
        project_id: None,
        start_at: T0,
        end_at: Some(T0 + HOUR),
        tz: Some("UTC".into()),
        note: String::new(),
        billable: None,
        tag_ids: vec![],
        cost_center_id: None,
    })
    .await
    .unwrap();
    assert!(events.try_recv().is_ok());
}
