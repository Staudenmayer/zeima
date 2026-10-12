#![cfg(feature = "db")]

use std::sync::Arc;

use chrono::{NaiveDate, TimeZone};
use time_core::autopause::{AutoPauseConfig, AwayCause, AwayPeriod};
use time_core::clock::ManualClock;
use time_core::model::{
    BudgetPeriod, EntryInput, PauseKind, ProjectInput, ProjectStatus, StartTimer, TimerState,
};
use time_core::summary::Range;
use time_core::worktime::parse_hhmm;
use time_core::{AwayResolution, Core};

fn at(day: &str, hhmm: &str) -> i64 {
    let local = day
        .parse::<NaiveDate>()
        .unwrap()
        .and_time(parse_hhmm(hhmm).unwrap());
    chrono_tz::Europe::Vienna
        .from_local_datetime(&local)
        .single()
        .unwrap()
        .timestamp_millis()
}

const DAY: &str = "2026-06-01";

async fn setup() -> (Core, Arc<ManualClock>, String) {
    let clock = Arc::new(ManualClock::new(at(DAY, "09:00")));
    let core = Core::open_in_memory(clock.clone()).await.unwrap();
    core.set_default_tz("Europe/Vienna").unwrap();
    let project = core
        .create_project(ProjectInput {
            name: "Client".into(),
            alias: Some("CL".into()),
            color: "#3b82f6".into(),
            parent_id: None,
            client: None,
            budget_minutes: None,
            budget_period: BudgetPeriod::Total,
            budget_warn_pct: 80,
            billable_default: false,
            status: ProjectStatus::Active,
            cost_center_id: None,
        })
        .await
        .unwrap();
    core.start_timer(StartTimer {
        project_id: Some(project.id.clone()),
        tag_ids: vec![],
        note: "work".into(),
        billable: None,
    })
    .await
    .unwrap();
    (core, clock, project.id)
}

/// `(start, end)` of all entries as local `HH:MM`.
async fn entries(core: &Core) -> Vec<(String, Option<String>)> {
    let fmt = |ms: i64| {
        chrono_tz::Europe::Vienna
            .timestamp_millis_opt(ms)
            .single()
            .unwrap()
            .format("%H:%M")
            .to_string()
    };
    core.list_entries(Range {
        from: 0,
        to: i64::MAX,
    })
    .await
    .unwrap()
    .into_iter()
    .map(|e| (fmt(e.start_at), e.end_at.map(fmt)))
    .collect()
}

fn s(v: &str) -> String {
    v.to_string()
}

fn period(from: &str, to: &str, cause: AwayCause) -> AwayPeriod {
    AwayPeriod {
        from: at(DAY, from),
        to: at(DAY, to),
        cause,
    }
}

#[tokio::test]
async fn lock_is_excluded_and_can_be_undone() {
    let (core, clock, _) = setup().await;
    clock.set(at(DAY, "10:30"));
    let cfg = AutoPauseConfig::default();
    let pause = core
        .handle_away(period("10:00", "10:30", AwayCause::Locked), &cfg)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pause.kind, PauseKind::AutoLock);
    assert!(pause.applied);
    assert_eq!(
        entries(&core).await,
        vec![(s("09:00"), Some(s("10:00"))), (s("10:30"), None)]
    );
    assert_eq!(core.pending_away().await.unwrap().len(), 1);

    core.resolve_away(&pause.id, AwayResolution::Keep).await.unwrap();
    assert_eq!(
        entries(&core).await,
        vec![(s("09:00"), None)],
        "merged back into one running entry"
    );
    assert!(core.pending_away().await.unwrap().is_empty());
}

#[tokio::test]
async fn idle_prompt_changes_nothing_until_excluded() {
    let (core, clock, _) = setup().await;
    clock.set(at(DAY, "10:20"));
    let cfg = AutoPauseConfig::default();
    let pause = core
        .handle_away(period("10:00", "10:20", AwayCause::Idle), &cfg)
        .await
        .unwrap()
        .unwrap();
    assert!(!pause.applied);
    assert_eq!(entries(&core).await, vec![(s("09:00"), None)]);

    core.resolve_away(&pause.id, AwayResolution::Exclude)
        .await
        .unwrap();
    assert_eq!(
        entries(&core).await,
        vec![(s("09:00"), Some(s("10:00"))), (s("10:20"), None)]
    );
    assert!(core.pending_away().await.unwrap().is_empty());
}

#[tokio::test]
async fn away_time_can_be_assigned_to_another_project() {
    let (core, clock, _) = setup().await;
    let other = core
        .create_project(ProjectInput {
            name: "Meeting".into(),
            alias: None,
            color: "#22c55e".into(),
            parent_id: None,
            client: None,
            budget_minutes: None,
            budget_period: BudgetPeriod::Total,
            budget_warn_pct: 80,
            billable_default: false,
            status: ProjectStatus::Active,
            cost_center_id: None,
        })
        .await
        .unwrap();
    clock.set(at(DAY, "10:20"));
    let pause = core
        .handle_away(
            period("10:00", "10:20", AwayCause::Idle),
            &AutoPauseConfig::default(),
        )
        .await
        .unwrap()
        .unwrap();
    core.resolve_away(
        &pause.id,
        AwayResolution::Assign {
            project_id: other.id.clone(),
        },
    )
    .await
    .unwrap();
    let list = core
        .list_entries(Range {
            from: 0,
            to: i64::MAX,
        })
        .await
        .unwrap();
    assert_eq!(
        entries(&core).await,
        vec![
            (s("09:00"), Some(s("10:00"))),
            (s("10:00"), Some(s("10:20"))),
            (s("10:20"), None),
        ]
    );
    assert_eq!(list[1].project_id.as_deref(), Some(other.id.as_str()));
}

#[tokio::test]
async fn timer_left_overnight_is_stopped_at_last_activity() {
    let (core, clock, _) = setup().await;
    clock.set(at("2026-06-02", "08:00"));
    let p = AwayPeriod {
        from: at(DAY, "18:30"),
        to: at("2026-06-02", "08:00"),
        cause: AwayCause::Suspended,
    };
    let pause = core
        .handle_away(p, &AutoPauseConfig::default())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pause.kind, PauseKind::AutoNight);
    assert_eq!(core.timer_state().await.unwrap(), TimerState::Idle);
    assert_eq!(entries(&core).await, vec![(s("09:00"), Some(s("18:30")))]);

    // "It was work" → the entry runs again.
    core.resolve_away(&pause.id, AwayResolution::Keep).await.unwrap();
    assert!(matches!(
        core.timer_state().await.unwrap(),
        TimerState::Running { .. }
    ));
}

#[tokio::test]
async fn forgotten_timer_after_app_was_closed() {
    let (core, clock, _) = setup().await;
    clock.set(at(DAY, "17:00"));
    core.heartbeat().await.unwrap();
    clock.set(at("2026-06-02", "08:30"));
    let pause = core
        .check_forgotten_timer(&AutoPauseConfig::default())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pause.kind, PauseKind::AutoNight);
    assert_eq!(entries(&core).await, vec![(s("09:00"), Some(s("17:00")))]);
    // A recent heartbeat is not a forgotten timer.
    core.start_timer(StartTimer {
        project_id: None,
        tag_ids: vec![],
        note: String::new(),
        billable: None,
    })
    .await
    .unwrap();
    core.heartbeat().await.unwrap();
    assert!(core
        .check_forgotten_timer(&AutoPauseConfig::default())
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn lunch_is_excluded_automatically() {
    let (core, clock, _) = setup().await;
    clock.set(at(DAY, "12:45"));
    let pause = core
        .handle_away(
            period("12:00", "12:45", AwayCause::Idle),
            &AutoPauseConfig::default(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(pause.kind, PauseKind::AutoLunch);
    assert_eq!(
        entries(&core).await,
        vec![(s("09:00"), Some(s("12:00"))), (s("12:45"), None)]
    );
}

#[tokio::test]
async fn nothing_happens_without_a_running_timer_or_when_disabled() {
    let (core, clock, _) = setup().await;
    clock.set(at(DAY, "10:30"));
    let off = AutoPauseConfig {
        enabled: false,
        ..AutoPauseConfig::default()
    };
    assert!(core
        .handle_away(period("10:00", "10:30", AwayCause::Locked), &off)
        .await
        .unwrap()
        .is_none());
    core.stop_timer().await.unwrap();
    let cfg = AutoPauseConfig::default();
    assert!(core
        .handle_away(period("10:00", "10:30", AwayCause::Locked), &cfg)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn undo_after_the_continuation_was_edited_fills_the_gap() {
    let (core, clock, project) = setup().await;
    clock.set(at(DAY, "10:30"));
    let pause = core
        .handle_away(
            period("10:00", "10:30", AwayCause::Locked),
            &AutoPauseConfig::default(),
        )
        .await
        .unwrap()
        .unwrap();
    // The user changes the note of the continuation before undoing.
    let next = core
        .get_entry(pause.followup_entry_id.as_deref().unwrap())
        .await
        .unwrap();
    core.update_entry(
        &next.id,
        EntryInput {
            project_id: Some(project),
            start_at: next.start_at,
            end_at: None,
            tz: None,
            note: "changed".into(),
            billable: None,
            tag_ids: vec![],
            cost_center_id: None,
        },
    )
    .await
    .unwrap();
    core.resolve_away(&pause.id, AwayResolution::Keep).await.unwrap();
    assert_eq!(
        entries(&core).await,
        vec![
            (s("09:00"), Some(s("10:00"))),
            (s("10:00"), Some(s("10:30"))),
            (s("10:30"), None),
        ]
    );
}
