#![cfg(feature = "db")]

//! Two devices and a stand-in server: the server keeps each row's merged
//! state, exactly as the real one does with the same merge function.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use zeima_core::clock::ManualClock;
use zeima_core::model::{BudgetPeriod, EntryInput, ProjectInput, ProjectStatus};
use zeima_core::sync::{merge, Actor, Outcome, RowState};
use zeima_core::{Core, RemoteRow, SYNC_BATCH};

#[derive(Default)]
struct Server {
    rows: BTreeMap<(String, String), RowState>,
}

impl Server {
    /// What a device's push does on the server.
    async fn push(&mut self, device: &Core) {
        let outbox = device.sync_outbox(SYNC_BATCH).await.unwrap();
        let mut results = Vec::new();
        for change in &outbox.changes {
            let key = (change.table.clone(), change.row_id.clone());
            let (state, outcome) = merge(self.rows.get(&key), change, Actor { manager: true });
            if outcome == Outcome::Applied {
                self.rows.insert(key, state);
            }
            results.push(outcome);
        }
        device.sync_mark_pushed(&outbox, &results).await.unwrap();
    }

    /// What a device's pull gets: every row (the tests do not need cursors).
    fn rows(&self) -> Vec<RemoteRow> {
        self.rows
            .iter()
            .map(|((table, id), state)| RemoteRow {
                workspace_id: "server-workspace".into(),
                table: table.clone(),
                row_id: id.clone(),
                fields: state.fields.clone(),
                deleted: state.deleted.clone(),
            })
            .collect()
    }
}

fn input(name: &str, alias: Option<&str>) -> ProjectInput {
    ProjectInput {
        name: name.into(),
        alias: alias.map(Into::into),
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

async fn device(start: i64) -> (Core, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new(start));
    (Core::open_in_memory(clock.clone()).await.unwrap(), clock)
}

#[tokio::test]
async fn a_row_made_on_one_device_appears_on_the_other_with_local_ids() {
    let (a, _) = device(1_700_000_000_000).await;
    let (b, _) = device(1_700_000_000_000).await;
    let mut server = Server::default();
    let project = a.create_project(input("Alpha", Some("AL"))).await.unwrap();
    server.push(&a).await;

    let report = b.sync_apply(&server.rows()).await.unwrap();
    assert!(report.applied >= 1);
    assert_eq!(report.conflicts, 0);

    let got = b.get_project(&project.id).await.unwrap();
    assert_eq!(got.name, "Alpha");
    assert_eq!(got.alias.as_deref(), Some("AL"));
    assert_ne!(
        got.workspace_id,
        a.ctx().workspace_id,
        "the row belongs to B's own workspace"
    );
    assert_eq!(got.workspace_id, b.ctx().workspace_id);
}

#[tokio::test]
async fn pulled_rows_are_not_pushed_back() {
    let (a, _) = device(1_700_000_000_000).await;
    let (b, _) = device(1_700_000_000_000).await;
    let mut server = Server::default();
    let project = a.create_project(input("Alpha", None)).await.unwrap();
    server.push(&a).await;
    b.sync_apply(&server.rows()).await.unwrap();
    // B's own set-up (the built-in absence types) is its to send; the pulled
    // project is not.
    let outbox = b.sync_outbox(SYNC_BATCH).await.unwrap();
    assert!(outbox.changes.iter().all(|c| c.row_id != project.id));
}

#[tokio::test]
async fn applying_the_same_rows_twice_changes_nothing() {
    let (a, _) = device(1_700_000_000_000).await;
    let (b, _) = device(1_700_000_000_000).await;
    let mut server = Server::default();
    a.create_project(input("Alpha", None)).await.unwrap();
    server.push(&a).await;
    assert!(b.sync_apply(&server.rows()).await.unwrap().applied >= 1);
    assert_eq!(b.sync_apply(&server.rows()).await.unwrap(), Default::default());
}

#[tokio::test]
async fn edits_to_different_fields_on_two_devices_both_survive() {
    let (a, clock_a) = device(1_700_000_000_000).await;
    let (b, clock_b) = device(1_700_000_000_000).await;
    let mut server = Server::default();
    let project = a.create_project(input("Alpha", None)).await.unwrap();
    server.push(&a).await;
    b.sync_apply(&server.rows()).await.unwrap();

    // Both edit while offline: A renames, B recolors.
    clock_a.advance(10_000);
    a.update_project(&project.id, input("Beta", None)).await.unwrap();
    clock_b.advance(20_000);
    let mut recolored = input("Alpha", None);
    recolored.color = "#ef4444".into();
    b.update_project(&project.id, recolored).await.unwrap();

    server.push(&a).await;
    server.push(&b).await;
    a.sync_apply(&server.rows()).await.unwrap();
    b.sync_apply(&server.rows()).await.unwrap();

    for device in [&a, &b] {
        let p = device.get_project(&project.id).await.unwrap();
        assert_eq!(p.name, "Beta", "A's rename");
        assert_eq!(p.color, "#ef4444", "B's recolor");
    }
}

#[tokio::test]
async fn an_unpushed_local_edit_beats_an_older_remote_one() {
    let (a, clock_a) = device(1_700_000_000_000).await;
    let (b, clock_b) = device(1_700_000_000_000).await;
    let mut server = Server::default();
    let project = a.create_project(input("Alpha", None)).await.unwrap();
    server.push(&a).await;
    b.sync_apply(&server.rows()).await.unwrap();

    clock_a.advance(10_000);
    a.update_project(&project.id, input("From A", None))
        .await
        .unwrap();
    server.push(&a).await;
    clock_b.advance(20_000);
    b.update_project(&project.id, input("From B", None))
        .await
        .unwrap(); // not pushed yet

    b.sync_apply(&server.rows()).await.unwrap();
    assert_eq!(b.get_project(&project.id).await.unwrap().name, "From B");
    server.push(&b).await;
    a.sync_apply(&server.rows()).await.unwrap();
    assert_eq!(a.get_project(&project.id).await.unwrap().name, "From B");
}

#[tokio::test]
async fn a_delete_reaches_the_other_device() {
    let (a, clock_a) = device(1_700_000_000_000).await;
    let (b, _) = device(1_700_000_000_000).await;
    let mut server = Server::default();
    let project = a.create_project(input("Alpha", None)).await.unwrap();
    server.push(&a).await;
    b.sync_apply(&server.rows()).await.unwrap();
    assert_eq!(b.list_projects(true).await.unwrap().len(), 1);

    clock_a.advance(10_000);
    a.delete_project(&project.id).await.unwrap();
    server.push(&a).await;
    b.sync_apply(&server.rows()).await.unwrap();
    assert!(b.list_projects(true).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_row_that_cannot_be_written_is_reported_and_the_rest_go_on() {
    let (a, _) = device(1_700_000_000_000).await;
    let (b, _) = device(1_700_000_000_000).await;
    let mut server = Server::default();
    a.create_project(input("Taken elsewhere", Some("X")))
        .await
        .unwrap();
    let ok = a.create_project(input("Fine", Some("OK"))).await.unwrap();
    server.push(&a).await;
    // B already has its own project with the alias X.
    b.create_project(input("Mine", Some("X"))).await.unwrap();

    let report = b.sync_apply(&server.rows()).await.unwrap();
    assert_eq!(report.conflicts, 1);
    assert_eq!(b.get_project(&ok.id).await.unwrap().name, "Fine");
    assert_eq!(b.list_projects(true).await.unwrap().len(), 2);
}

#[tokio::test]
async fn the_clock_moves_past_what_the_server_sent() {
    let (a, _) = device(2_000_000_000_000).await;
    let (b, _) = device(1_000_000_000_000).await; // B's clock is far behind
    let mut server = Server::default();
    let project = a.create_project(input("Alpha", None)).await.unwrap();
    server.push(&a).await;
    b.sync_apply(&server.rows()).await.unwrap();

    // B's next edit must sort after A's create even though B's wall clock is behind.
    b.update_project(&project.id, input("Beta", None)).await.unwrap();
    server.push(&b).await;
    a.sync_apply(&server.rows()).await.unwrap();
    assert_eq!(a.get_project(&project.id).await.unwrap().name, "Beta");
}

#[tokio::test]
async fn a_conflict_can_be_retried_after_the_blocker_is_gone() {
    let (a, _) = device(1_700_000_000_000).await;
    let (b, _) = device(1_700_000_000_000).await;
    let mut server = Server::default();
    let theirs = a.create_project(input("Theirs", Some("X"))).await.unwrap();
    server.push(&a).await;
    let mine = b.create_project(input("Mine", Some("X"))).await.unwrap();

    assert_eq!(b.sync_apply(&server.rows()).await.unwrap().conflicts, 1);
    let conflicts = b.sync_conflicts().await.unwrap();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].row_id, theirs.id);
    assert_eq!(conflicts[0].name.as_deref(), Some("Theirs"));
    assert!(!conflicts[0].message.is_empty());

    // Still blocked: retrying keeps the conflict.
    assert!(!b.sync_retry_conflict("project", &theirs.id).await.unwrap());
    assert_eq!(b.sync_conflicts().await.unwrap().len(), 1);

    // The user gives their own project another alias; now the retry works.
    b.update_project(&mine.id, input("Mine", Some("Y")))
        .await
        .unwrap();
    assert!(b.sync_retry_conflict("project", &theirs.id).await.unwrap());
    assert!(b.sync_conflicts().await.unwrap().is_empty());
    assert_eq!(
        b.get_project(&theirs.id).await.unwrap().alias.as_deref(),
        Some("X")
    );
}

#[tokio::test]
async fn a_conflict_can_be_dismissed() {
    let (a, _) = device(1_700_000_000_000).await;
    let (b, _) = device(1_700_000_000_000).await;
    let mut server = Server::default();
    let theirs = a.create_project(input("Theirs", Some("X"))).await.unwrap();
    server.push(&a).await;
    b.create_project(input("Mine", Some("X"))).await.unwrap();
    b.sync_apply(&server.rows()).await.unwrap();

    b.sync_dismiss_conflict("project", &theirs.id).await.unwrap();
    assert!(b.sync_conflicts().await.unwrap().is_empty());
    assert_eq!(b.sync_conflict_count().await.unwrap(), 0);
}

#[tokio::test]
async fn wiping_the_local_copy_lets_the_server_rebuild_it() {
    let (a, _) = device(1_700_000_000_000).await;
    let (b, _) = device(1_700_000_000_000).await;
    let mut server = Server::default();
    let theirs = a.create_project(input("Theirs", Some("X"))).await.unwrap();
    server.push(&a).await;
    // B made its own project with the same alias before joining.
    let mine = b.create_project(input("Mine", Some("X"))).await.unwrap();
    assert_eq!(b.sync_apply(&server.rows()).await.unwrap().conflicts, 1);

    b.sync_wipe_local().await.unwrap();
    assert!(b.list_projects(true).await.unwrap().is_empty());
    assert!(
        b.sync_outbox(SYNC_BATCH).await.unwrap().is_empty(),
        "its own history is gone"
    );
    assert!(b.sync_conflicts().await.unwrap().is_empty());
    assert_eq!(b.sync_cursor().await.unwrap(), 0);

    let report = b.sync_apply(&server.rows()).await.unwrap();
    assert_eq!(report.conflicts, 0);
    assert_eq!(b.get_project(&theirs.id).await.unwrap().name, "Theirs");
    assert!(b.get_project(&mine.id).await.is_err());
}

fn entry_input(start: i64, end: i64) -> EntryInput {
    EntryInput {
        activity_id: common::activity(),
        project_id: None,
        start_at: start,
        end_at: Some(end),
        tz: None,
        note: String::new(),
        billable: None,
        tag_ids: vec![],
        cost_center_id: None,
    }
}

#[tokio::test]
async fn an_entrys_start_and_end_stay_a_consistent_pair() {
    const H: i64 = 3_600_000;
    let t0 = 1_700_000_000_000;
    let (a, clock_a) = device(t0).await;
    let (b, clock_b) = device(t0).await;
    let a = common::with_activity(a).await;
    let mut server = Server::default();

    let entry = a.create_entry(entry_input(t0, t0 + 8 * H)).await.unwrap();
    server.push(&a).await;
    b.sync_apply(&server.rows()).await.unwrap();
    let seen = b.get_entry(&entry.id).await.unwrap();
    assert_eq!((seen.start_at, seen.end_at), (t0, Some(t0 + 8 * H)));

    // Offline: a moves the end earlier, b moves the start later. Each is fine
    // alone; combined field by field they would give an end before the start.
    clock_a.advance(10_000);
    a.update_entry(&entry.id, entry_input(t0, t0 + 2 * H))
        .await
        .unwrap();
    clock_b.advance(20_000);
    b.update_entry(&entry.id, entry_input(t0 + 6 * H, t0 + 8 * H))
        .await
        .unwrap();

    server.push(&a).await;
    server.push(&b).await;
    assert_eq!(a.sync_apply(&server.rows()).await.unwrap().conflicts, 0);
    assert_eq!(b.sync_apply(&server.rows()).await.unwrap().conflicts, 0);

    let (ea, eb) = (
        a.get_entry(&entry.id).await.unwrap(),
        b.get_entry(&entry.id).await.unwrap(),
    );
    assert_eq!(
        (ea.start_at, ea.end_at),
        (eb.start_at, eb.end_at),
        "both devices agree"
    );
    assert!(
        ea.end_at.unwrap() >= ea.start_at,
        "and the pair is one device's edit, not a mix"
    );
    // b's was the later edit, so its pair wins as a whole.
    assert_eq!((ea.start_at, ea.end_at), (t0 + 6 * H, Some(t0 + 8 * H)));
}

#[tokio::test]
async fn a_conflict_can_be_compared_field_by_field() {
    let (a, _) = device(1_700_000_000_000).await;
    let (b, _) = device(1_700_000_000_000).await;
    let mut server = Server::default();
    let theirs = a.create_project(input("Theirs", Some("X"))).await.unwrap();
    server.push(&a).await;
    b.create_project(input("Mine", Some("X"))).await.unwrap();
    b.sync_apply(&server.rows()).await.unwrap();

    // The row is not on b (the alias is taken), so there is nothing local to show.
    let view = b.sync_conflict_compare("project", &theirs.id).await.unwrap();
    let name = view.iter().find(|f| f.field == "name").unwrap();
    assert_eq!(name.server, serde_json::json!("Theirs"));
    assert_eq!(name.local, None);
    assert!(view
        .iter()
        .all(|f| f.field != "workspaceId" && f.field != "updatedAt"));

    // A row that exists on both sides shows both values.
    let both = a.create_project(input("Shared", None)).await.unwrap();
    server.push(&a).await;
    b.sync_apply(&server.rows()).await.unwrap();
    b.update_project(&both.id, input("Shared, edited here", None))
        .await
        .unwrap();
    let view = b.sync_conflict_compare("project", &both.id).await.unwrap();
    let name = view.iter().find(|f| f.field == "name").unwrap();
    assert_eq!(name.server, serde_json::json!("Shared"));
    assert_eq!(name.local, Some(serde_json::json!("Shared, edited here")));
    let billable = view.iter().find(|f| f.field == "billableDefault").unwrap();
    assert_eq!(
        billable.local,
        Some(serde_json::json!(0)),
        "booleans are stored as 0/1"
    );
}

fn start(activity: Option<String>, note: &str) -> zeima_core::model::StartTimer {
    zeima_core::model::StartTimer {
        activity_id: activity,
        project_id: None,
        tag_ids: vec![],
        note: note.into(),
        billable: None,
    }
}

#[tokio::test]
async fn a_timer_started_on_one_device_runs_on_the_other_and_stops_there() {
    use zeima_core::model::TimerState;
    let t0 = 1_700_000_000_000;
    let (a, clock_a) = device(t0).await;
    let (b, clock_b) = device(t0).await;
    let a = common::with_activity(a).await;
    let mut server = Server::default();
    server.push(&a).await;
    b.sync_apply(&server.rows()).await.unwrap();

    a.start_timer(start(common::activity(), "writing")).await.unwrap();
    server.push(&a).await;
    assert_eq!(b.sync_apply(&server.rows()).await.unwrap().conflicts, 0);
    let TimerState::Running { entry } = b.timer_state().await.unwrap() else {
        panic!("the timer should be running on b");
    };
    assert_eq!(entry.note, "writing");

    // b stops it; a sees it stopped.
    clock_b.advance(60_000);
    clock_a.advance(60_000);
    b.stop_timer().await.unwrap();
    server.push(&b).await;
    assert_eq!(a.sync_apply(&server.rows()).await.unwrap().conflicts, 0);
    assert!(matches!(a.timer_state().await.unwrap(), TimerState::Idle));
    let stopped = a.get_entry(&entry.id).await.unwrap();
    assert!(stopped.end_at.is_some());
}

#[tokio::test]
async fn two_timers_started_offline_are_a_conflict_with_clear_advice() {
    use zeima_core::model::TimerState;
    let t0 = 1_700_000_000_000;
    let (a, _) = device(t0).await;
    let (b, clock_b) = device(t0).await;
    let a = common::with_activity(a).await;
    let mut server = Server::default();
    server.push(&a).await;
    b.sync_apply(&server.rows()).await.unwrap();

    // Both start a timer while offline.
    let TimerState::Running { entry: on_a } = a.start_timer(start(common::activity(), "on a")).await.unwrap()
    else {
        panic!("a's timer should be running");
    };
    clock_b.advance(5_000);
    b.start_timer(start(common::activity(), "on b")).await.unwrap();
    server.push(&a).await;
    server.push(&b).await;

    // Each device keeps its own timer and is told about the other.
    assert_eq!(a.sync_apply(&server.rows()).await.unwrap().conflicts, 1);
    assert_eq!(b.sync_apply(&server.rows()).await.unwrap().conflicts, 1);
    assert!(matches!(
        b.timer_state().await.unwrap(),
        TimerState::Running { .. }
    ));
    let conflicts = b.sync_conflicts().await.unwrap();
    assert!(
        conflicts[0].message.contains("timer is already running"),
        "{}",
        conflicts[0].message
    );

    // Following the advice: b stops its timer, then retries; a's timer is then the one running on b.
    b.stop_timer().await.unwrap();
    assert!(b.sync_retry_conflict("time_entry", &on_a.id).await.unwrap());
    let TimerState::Running { entry } = b.timer_state().await.unwrap() else {
        panic!("a's timer should run on b after the retry");
    };
    assert_eq!(entry.note, "on a");
}
