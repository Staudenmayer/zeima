#![cfg(feature = "db")]

//! Sync of an organization's shared workspace between two devices: changes
//! go out per workspace, the account id travels with the rows, and rows of
//! other people arrive with their owner.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use sqlx::Row;
use zeima_core::clock::ManualClock;
use zeima_core::model::{ActivityInput, BudgetPeriod, EntryInput, ProjectInput, ProjectStatus};
use zeima_core::sync::{merge, Actor, Outcome, RowState};
use zeima_core::{Core, RemoteRow, SYNC_BATCH};

const T0: i64 = 1_791_187_200_000;
const HOUR: i64 = 3_600_000;
const WS: &str = "0192f000-0000-7000-8000-00000000a001";
const ORG: &str = "0192f000-0000-7000-8000-00000000b001";
const ADA: &str = "0192f000-0000-7000-8000-00000000c001";
const BEN: &str = "0192f000-0000-7000-8000-00000000c002";

#[derive(Default)]
struct Server {
    rows: BTreeMap<(String, String), RowState>,
    /// What each change looked like on the wire.
    sent: Vec<zeima_core::sync::Change>,
}

impl Server {
    async fn push(&mut self, workspace: &Core) -> usize {
        let mut n = 0;
        loop {
            let outbox = workspace.sync_outbox(SYNC_BATCH).await.unwrap();
            if outbox.is_empty() {
                return n;
            }
            let mut results = Vec::new();
            for change in &outbox.changes {
                let key = (change.table.clone(), change.row_id.clone());
                let (state, outcome) = merge(self.rows.get(&key), change, Actor { manager: true });
                if outcome == Outcome::Applied {
                    self.rows.insert(key, state);
                }
                self.sent.push(change.clone());
                results.push(outcome);
            }
            n += outbox.changes.len();
            workspace.sync_mark_pushed(&outbox, &results).await.unwrap();
        }
    }

    async fn pull(&self, workspace: &Core) {
        let rows: Vec<RemoteRow> = self
            .rows
            .iter()
            .map(|((table, id), state)| RemoteRow {
                workspace_id: WS.into(),
                table: table.clone(),
                row_id: id.clone(),
                fields: state.fields.clone(),
                deleted: state.deleted.clone(),
            })
            .collect();
        workspace.sync_apply(&rows).await.unwrap();
    }
}

/// A device: its personal core and its handle on the shared workspace.
async fn device(user: &str, name: &str) -> (Core, Core) {
    let personal = common::with_activity(
        Core::open_in_memory(Arc::new(ManualClock::new(T0)))
            .await
            .unwrap(),
    )
    .await;
    personal
        .adopt_org_workspace(WS, ORG, "Acme", user, name)
        .await
        .unwrap();
    let org = personal.scoped(WS, user);
    (personal, org)
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
    }
}

#[tokio::test]
async fn changes_go_out_per_workspace() {
    let (personal, org) = device(ADA, "Ada").await;
    personal.create_project(project("Mine")).await.unwrap();
    org.create_project(project("Shared")).await.unwrap();

    let mut server = Server::default();
    // The shared workspace's changes are sent from its own handle only.
    assert_eq!(server.push(&org).await, 1);
    let sent = &server.sent[0].fields;
    assert_eq!(sent["name"], "Shared");
    // The personal workspace still has its own change waiting.
    assert!(server.push(&personal).await >= 2); // "Mine" and the defaults a new install creates
    assert!(server
        .sent
        .iter()
        .any(|c| c.fields.get("name") == Some(&"Mine".into())));
}

#[tokio::test]
async fn shared_rows_carry_their_owner_and_arrive_on_the_other_device() {
    let (_, ada) = device(ADA, "Ada").await;
    let (ben_personal, ben) = device(BEN, "Ben").await;
    let activity = ada
        .create_activity(ActivityInput {
            name: "Work".into(),
            color: "#111111".into(),
            archived: false,
        })
        .await
        .unwrap();
    ada.create_entry(EntryInput {
        activity_id: Some(activity.id.clone()),
        project_id: None,
        start_at: T0,
        end_at: Some(T0 + HOUR),
        tz: Some("UTC".into()),
        note: "Ada's work".into(),
        billable: None,
        tag_ids: vec![],
        cost_center_id: None,
    })
    .await
    .unwrap();

    let mut server = Server::default();
    server.push(&ada).await;
    let entry = server.sent.iter().find(|c| c.table == "time_entry").unwrap();
    let fields = &entry.fields;
    assert_eq!(fields["userId"], ADA, "the account id is sent");
    assert!(!fields.contains_key("workspaceId"));

    server.pull(&ben).await;
    // Ben's device now has the activity and Ada's entry, owned by Ada.
    let owner: String = sqlx::query("SELECT user_id FROM time_entry WHERE note = 'Ada''s work'")
        .fetch_one(ben_personal.pool())
        .await
        .unwrap()
        .get("user_id");
    assert_eq!(owner, ADA);
    let known: i64 = sqlx::query("SELECT count(*) AS n FROM app_user WHERE id = ?")
        .bind(ADA)
        .fetch_one(ben_personal.pool())
        .await
        .unwrap()
        .get("n");
    assert_eq!(known, 1, "Ada is known on Ben's device");
    let in_workspace: String = sqlx::query("SELECT workspace_id FROM time_entry WHERE note = 'Ada''s work'")
        .fetch_one(ben_personal.pool())
        .await
        .unwrap()
        .get("workspace_id");
    assert_eq!(in_workspace, WS);
    // Applying a pull does not queue anything to send back.
    assert_eq!(server.push(&ben).await, 0);
}
