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
