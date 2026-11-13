#![cfg(feature = "db")]

use std::sync::Arc;

use serde_json::json;
use zeima_core::clock::ManualClock;
use zeima_core::model::{BudgetPeriod, ProjectInput, ProjectStatus};
use zeima_core::sync::{Op, Outcome, Reason};
use zeima_core::{Core, SYNC_BATCH};

fn input(name: &str) -> ProjectInput {
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
    }
}

async fn core() -> (Core, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new(1_700_000_000_000));
    (Core::open_in_memory(clock.clone()).await.unwrap(), clock)
}

fn applied(n: usize) -> Vec<Outcome> {
    vec![Outcome::Applied; n]
}

#[tokio::test]
async fn a_new_row_is_sent_with_all_its_fields() {
    let (core, _) = core().await;
    let project = core.create_project(input("Alpha")).await.unwrap();
    let outbox = core.sync_outbox(SYNC_BATCH).await.unwrap();

    let change = outbox.changes.iter().find(|c| c.row_id == project.id).unwrap();
    assert_eq!(change.table, "project");
    assert_eq!(change.op, Op::Upsert);
    assert_eq!(change.fields["name"], json!("Alpha"));
    assert!(!change.fields.contains_key("id"));
    assert!(change.fields.len() > 5);
}

#[tokio::test]
async fn an_edit_sends_only_the_fields_that_changed() {
    let (core, clock) = core().await;
    let project = core.create_project(input("Alpha")).await.unwrap();
    let first = core.sync_outbox(SYNC_BATCH).await.unwrap();
    core.sync_mark_pushed(&first, &applied(first.changes.len()))
        .await
        .unwrap();
    assert!(core.sync_outbox(SYNC_BATCH).await.unwrap().changes.is_empty());

    clock.advance(1_000);
    core.update_project(&project.id, input("Beta")).await.unwrap();
    let second = core.sync_outbox(SYNC_BATCH).await.unwrap();
    assert_eq!(second.changes.len(), 1);
    let mut names: Vec<_> = second.changes[0].fields.keys().cloned().collect();
    names.sort();
    assert_eq!(names, ["name", "updatedAt"]);
}

#[tokio::test]
async fn edits_made_before_a_push_diff_against_each_other() {
    let (core, clock) = core().await;
    let project = core.create_project(input("Alpha")).await.unwrap();
    clock.advance(1_000);
    core.update_project(&project.id, input("Beta")).await.unwrap();

    let outbox = core.sync_outbox(SYNC_BATCH).await.unwrap();
    let mine: Vec<_> = outbox.changes.iter().filter(|c| c.row_id == project.id).collect();
    assert_eq!(mine.len(), 2);
    assert!(mine[0].fields.len() > 5);
    assert_eq!(mine[1].fields.len(), 2); // name and updatedAt
    assert_eq!(mine[1].fields["name"], json!("Beta"));
}

#[tokio::test]
async fn a_delete_is_sent_as_a_delete() {
    let (core, clock) = core().await;
    let project = core.create_project(input("Alpha")).await.unwrap();
    let first = core.sync_outbox(SYNC_BATCH).await.unwrap();
    core.sync_mark_pushed(&first, &applied(first.changes.len()))
        .await
        .unwrap();

    clock.advance(1_000);
    core.delete_project(&project.id).await.unwrap();
    let outbox = core.sync_outbox(SYNC_BATCH).await.unwrap();
    let change = outbox.changes.iter().find(|c| c.row_id == project.id).unwrap();
    assert_eq!(change.op, Op::Delete);
    assert!(change.fields.is_empty());
}

#[tokio::test]
async fn pushed_changes_are_not_sent_again() {
    let (core, _) = core().await;
    core.create_project(input("Alpha")).await.unwrap();
    let outbox = core.sync_outbox(SYNC_BATCH).await.unwrap();
    let before = core.pending_changes().await.unwrap();
    core.sync_mark_pushed(&outbox, &applied(outbox.changes.len()))
        .await
        .unwrap();
    assert!(core.pending_changes().await.unwrap() < before);
    assert!(core.sync_outbox(SYNC_BATCH).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_rejected_change_is_not_retried_and_not_remembered() {
    let (core, clock) = core().await;
    let project = core.create_project(input("Alpha")).await.unwrap();
    let first = core.sync_outbox(SYNC_BATCH).await.unwrap();
    let n = first.changes.len();
    core.sync_mark_pushed(
        &first,
        &vec![
            Outcome::Rejected {
                reason: Reason::Locked
            };
            n
        ],
    )
    .await
    .unwrap();
    assert!(core.sync_outbox(SYNC_BATCH).await.unwrap().is_empty());

    // The server never took it, so the next edit still sends every field it
    // differs on, not just the new one.
    clock.advance(1_000);
    core.update_project(&project.id, input("Beta")).await.unwrap();
    let next = core.sync_outbox(SYNC_BATCH).await.unwrap();
    assert!(next.changes[0].fields.len() > 5);
}

#[tokio::test]
async fn the_server_must_answer_every_change() {
    let (core, _) = core().await;
    core.create_project(input("Alpha")).await.unwrap();
    let outbox = core.sync_outbox(SYNC_BATCH).await.unwrap();
    assert!(core.sync_mark_pushed(&outbox, &[]).await.is_err());
}
