#![cfg(feature = "db")]

use std::sync::Arc;

use time_core::clock::ManualClock;
use time_core::model::{
    BudgetPeriod, EntryInput, ProjectInput, ProjectStatus, StartTimer, TagInput, TimerState,
};
use time_core::summary::Range;
use time_core::{Core, Error};

const MIN: i64 = 60_000;
const T0: i64 = 1_700_000_000_000;

async fn setup() -> (Core, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new(T0));
    (Core::open_in_memory(clock.clone()).await.unwrap(), clock)
}

fn project(name: &str, alias: Option<&str>) -> ProjectInput {
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

fn tag(name: &str, project_id: Option<&str>, group: Option<&str>) -> TagInput {
    TagInput {
        name: name.into(),
        color: "#22c55e".into(),
        project_id: project_id.map(Into::into),
        group_name: group.map(Into::into),
    }
}

fn manual(project_id: Option<&str>, start: i64, end: i64, tag_ids: Vec<String>) -> EntryInput {
    EntryInput {
        project_id: project_id.map(Into::into),
        start_at: start,
        end_at: Some(end),
        tz: Some("Europe/Vienna".into()),
        note: String::new(),
        billable: None,
        tag_ids,
        cost_center_id: None,
    }
}

#[tokio::test]
async fn project_alias_is_unique_case_insensitively() {
    let (core, _) = setup().await;
    core.create_project(project("Internal", Some("INT")))
        .await
        .unwrap();
    let err = core
        .create_project(project("Other", Some("int")))
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Validation { ref field, .. } if field == "alias"),
        "{err:?}"
    );
}

#[tokio::test]
async fn project_with_entries_cannot_be_deleted_but_can_be_archived() {
    let (core, _) = setup().await;
    let p = core.create_project(project("Client", Some("CL"))).await.unwrap();
    core.create_entry(manual(Some(&p.id), T0, T0 + MIN, vec![]))
        .await
        .unwrap();
    assert!(matches!(
        core.delete_project(&p.id).await,
        Err(Error::Conflict(_))
    ));

    let archived = core
        .update_project(
            &p.id,
            ProjectInput {
                status: ProjectStatus::Archived,
                ..project("Client", Some("CL"))
            },
        )
        .await
        .unwrap();
    assert_eq!(archived.status, ProjectStatus::Archived);
    assert!(core.list_projects(false).await.unwrap().is_empty());
    assert_eq!(core.list_projects(true).await.unwrap().len(), 1);
}

#[tokio::test]
async fn deleting_a_project_deletes_its_scoped_tags() {
    let (core, _) = setup().await;
    let p = core.create_project(project("P", None)).await.unwrap();
    core.create_tag(tag("Design", Some(&p.id), None)).await.unwrap();
    core.create_tag(tag("Global", None, None)).await.unwrap();
    core.delete_project(&p.id).await.unwrap();
    let names: Vec<_> = core
        .list_tags()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names, vec!["Global"]);
}

#[tokio::test]
async fn project_parent_cycles_are_rejected() {
    let (core, _) = setup().await;
    let a = core.create_project(project("A", None)).await.unwrap();
    let b = core
        .create_project(ProjectInput {
            parent_id: Some(a.id.clone()),
            ..project("B", None)
        })
        .await
        .unwrap();
    let err = core
        .update_project(
            &a.id,
            ProjectInput {
                parent_id: Some(b.id.clone()),
                ..project("A", None)
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Validation { ref field, .. } if field == "parentId"));
}

#[tokio::test]
async fn project_scoped_tags_only_apply_to_their_project() {
    let (core, _) = setup().await;
    let a = core.create_project(project("A", None)).await.unwrap();
    let b = core.create_project(project("B", None)).await.unwrap();
    let design = core.create_tag(tag("Design", Some(&a.id), None)).await.unwrap();

    core.create_entry(manual(Some(&a.id), T0, T0 + MIN, vec![design.id.clone()]))
        .await
        .unwrap();
    let err = core
        .create_entry(manual(Some(&b.id), T0, T0 + MIN, vec![design.id.clone()]))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Validation { ref field, .. } if field == "tagIds"));
}

#[tokio::test]
async fn only_one_tag_per_group() {
    let (core, _) = setup().await;
    let dev = core
        .create_tag(tag("Development", None, Some("Activity")))
        .await
        .unwrap();
    let meet = core
        .create_tag(tag("Meeting", None, Some("Activity")))
        .await
        .unwrap();
    let err = core
        .create_entry(manual(None, T0, T0 + MIN, vec![dev.id, meet.id]))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Validation { ref field, .. } if field == "tagIds"));
}

#[tokio::test]
async fn deleted_tags_disappear_from_entries() {
    let (core, _) = setup().await;
    let t = core.create_tag(tag("Temp", None, None)).await.unwrap();
    let e = core
        .create_entry(manual(None, T0, T0 + MIN, vec![t.id.clone()]))
        .await
        .unwrap();
    assert_eq!(e.tag_ids, vec![t.id.clone()]);
    core.delete_tag(&t.id).await.unwrap();
    assert!(core.get_entry(&e.id).await.unwrap().tag_ids.is_empty());
}

#[tokio::test]
async fn manual_entries_need_an_end() {
    let (core, _) = setup().await;
    let input = EntryInput {
        end_at: None,
        ..manual(None, T0, T0, vec![])
    };
    assert!(matches!(
        core.create_entry(input).await,
        Err(Error::Validation { .. })
    ));
}

#[tokio::test]
async fn split_running_entry_keeps_second_part_running() {
    let (core, clock) = setup().await;
    let p = core.create_project(project("P", None)).await.unwrap();
    let TimerState::Running { entry } = core
        .start_timer(StartTimer {
            project_id: Some(p.id.clone()),
            tag_ids: vec![],
            note: "x".into(),
            billable: None,
        })
        .await
        .unwrap()
    else {
        panic!()
    };
    clock.advance(60 * MIN);
    let (first, second) = core.split_entry(&entry.id, T0 + 20 * MIN).await.unwrap();
    assert_eq!(first.end_at, Some(T0 + 20 * MIN));
    assert_eq!(second.start_at, T0 + 20 * MIN);
    assert!(second.is_running());
    assert_eq!(second.project_id, first.project_id);

    let TimerState::Running { entry: running } = core.timer_state().await.unwrap() else {
        panic!("second part should still be running")
    };
    assert_eq!(running.id, second.id);
}

#[tokio::test]
async fn split_outside_the_entry_is_rejected() {
    let (core, _) = setup().await;
    let e = core
        .create_entry(manual(None, T0, T0 + 10 * MIN, vec![]))
        .await
        .unwrap();
    assert!(core.split_entry(&e.id, T0).await.is_err());
    assert!(core.split_entry(&e.id, T0 + 10 * MIN).await.is_err());
}

#[tokio::test]
async fn duplicate_appends_a_copy_after_the_original() {
    let (core, _) = setup().await;
    let e = core
        .create_entry(manual(None, T0, T0 + 30 * MIN, vec![]))
        .await
        .unwrap();
    let copy = core.duplicate_entry(&e.id).await.unwrap();
    assert_eq!(copy.start_at, T0 + 30 * MIN);
    assert_eq!(copy.end_at, Some(T0 + 60 * MIN));
    assert_ne!(copy.id, e.id);
}

#[tokio::test]
async fn summaries_per_day() {
    let (core, _) = setup().await;
    let p = core.create_project(project("P", None)).await.unwrap();
    core.create_entry(manual(Some(&p.id), T0, T0 + 90 * MIN, vec![]))
        .await
        .unwrap();
    core.create_entry(manual(None, T0 + 60 * MIN, T0 + 120 * MIN, vec![]))
        .await
        .unwrap();
    let day = 24 * 60 * MIN;
    let s = core
        .summaries(vec![
            Range {
                from: T0,
                to: T0 + day,
            },
            Range {
                from: T0 + day,
                to: T0 + 2 * day,
            },
        ])
        .await
        .unwrap();
    assert_eq!(s[0].total_ms, 120 * MIN, "overlap counted once");
    assert_eq!(s[0].by_project[0].ms, 90 * MIN);
    assert_eq!(s[1].total_ms, 0);
}

#[tokio::test]
async fn domain_events_are_published_after_commit() {
    use time_core::events::DomainEvent;
    let (core, clock) = setup().await;
    let mut events = core.subscribe();
    core.start_timer(StartTimer {
        project_id: None,
        tag_ids: vec![],
        note: String::new(),
        billable: None,
    })
    .await
    .unwrap();
    clock.advance(10 * MIN);
    core.stop_timer().await.unwrap();
    let mut hooks = vec![];
    while let Ok(e) = events.try_recv() {
        hooks.push(e.hook());
    }
    assert_eq!(
        hooks,
        vec!["entry.created", "entry.started", "entry.updated", "entry.stopped"]
    );

    // A failed write publishes nothing.
    let err = core
        .create_entry(manual(Some("missing"), T0, T0 + MIN, vec![]))
        .await;
    assert!(err.is_err());
    assert!(events.try_recv().is_err());

    let e = core
        .create_entry(manual(None, T0, T0 + MIN, vec![]))
        .await
        .unwrap();
    core.delete_entry(&e.id).await.unwrap();
    let hooks: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    assert!(matches!(hooks[0], DomainEvent::EntryCreated { .. }));
    assert!(matches!(&hooks[1], DomainEvent::EntryDeleted { entry_id } if *entry_id == e.id));
}
