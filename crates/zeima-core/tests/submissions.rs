#![cfg(feature = "db")]

mod common;

use std::sync::Arc;

use zeima_core::clock::ManualClock;
use zeima_core::model::BalanceAdjustmentInput;
use zeima_core::{Core, Error, SYNC_BATCH};

const T0: i64 = 1_791_187_200_000;
const WS: &str = "0192f000-0000-7000-8000-00000000a001";
const ORG: &str = "0192f000-0000-7000-8000-00000000b001";
const ADA: &str = "0192f000-0000-7000-8000-00000000c001";
const BEN: &str = "0192f000-0000-7000-8000-00000000c002";

fn date(s: &str) -> chrono::NaiveDate {
    s.parse().unwrap()
}

/// One database, two people in the shared workspace (as two handles).
async fn setup() -> (Core, Core, Core) {
    let personal = common::with_activity(
        Core::open_in_memory(Arc::new(ManualClock::new(T0)))
            .await
            .unwrap(),
    )
    .await;
    personal
        .adopt_org_workspace(WS, ORG, "Acme", ADA, "Ada")
        .await
        .unwrap();
    personal
        .adopt_org_workspace(WS, ORG, "Acme", BEN, "Ben")
        .await
        .unwrap();
    let (ada, ben) = (personal.scoped(WS, ADA), personal.scoped(WS, BEN));
    (personal, ada, ben)
}

#[tokio::test]
async fn a_persons_own_rows_carry_their_account_in_a_shared_workspace() {
    let (personal, ada, _) = setup().await;
    personal
        .create_adjustment(BalanceAdjustmentInput {
            date: date("2026-10-05"),
            minutes: 30,
            reason: "mine".into(),
        })
        .await
        .unwrap();
    ada.create_adjustment(BalanceAdjustmentInput {
        date: date("2026-10-05"),
        minutes: 60,
        reason: "shared".into(),
    })
    .await
    .unwrap();
    let in_org = ada.sync_outbox(SYNC_BATCH).await.unwrap();
    let adjustment = in_org
        .changes
        .iter()
        .find(|c| c.table == "balance_adjustment")
        .unwrap();
    assert_eq!(adjustment.fields["userId"], ADA);
    // The personal workspace keeps its local ids to itself.
    let own = personal.sync_outbox(SYNC_BATCH).await.unwrap();
    let adjustment = own
        .changes
        .iter()
        .find(|c| c.table == "balance_adjustment")
        .unwrap();
    assert!(!adjustment.fields.contains_key("userId"));
}

#[tokio::test]
async fn absences_in_a_shared_workspace_are_requests_someone_else_decides() {
    use zeima_core::model::{AbsenceInput, AbsenceStatus, AbsenceTypeInput};
    let (personal, ada, ben) = setup().await;
    let kind = ben
        .save_absence_type(
            None,
            AbsenceTypeInput {
                name: "Vacation".into(),
                color: "#22c55e".into(),
                reduces_target: true,
                affects_vacation: true,
            },
        )
        .await
        .unwrap();
    let request = |from: &str, to: &str| AbsenceInput {
        type_id: kind.id.clone(),
        start_date: date(from),
        end_date: date(to),
        half_day: None,
        note: String::new(),
    };

    let mine = ada
        .save_absence(None, request("2026-10-19", "2026-10-21"))
        .await
        .unwrap();
    assert_eq!(mine.status, AbsenceStatus::Requested);
    // The request waits for somebody else, who sees it; Ada does not see her own there.
    assert!(ada.list_absence_requests().await.unwrap().is_empty());
    assert_eq!(ben.list_absence_requests().await.unwrap().len(), 1);
    assert!(matches!(
        ada.decide_absence(&mine.id, true, "").await,
        Err(Error::Validation { .. })
    ));

    let approved = ben.decide_absence(&mine.id, true, "enjoy").await.unwrap();
    assert_eq!(
        (
            approved.status,
            approved.decided_by.as_deref(),
            approved.decision_note.as_str()
        ),
        (AbsenceStatus::Approved, Some(BEN), "enjoy")
    );
    assert!(matches!(
        ben.decide_absence(&mine.id, false, "").await,
        Err(Error::Conflict(_))
    ));

    // Changing an approved absence asks again and forgets the decision.
    let moved = ada
        .save_absence(Some(&mine.id), request("2026-10-20", "2026-10-22"))
        .await
        .unwrap();
    assert_eq!((moved.status, moved.decided_by), (AbsenceStatus::Requested, None));

    // A rejected request stays visible to the person, with the reason.
    let second = ada
        .save_absence(None, request("2026-11-02", "2026-11-03"))
        .await
        .unwrap();
    ben.decide_absence(&second.id, false, "busy week").await.unwrap();
    let rejected = ada
        .list_absences(date("2026-11-01"), date("2026-11-30"))
        .await
        .unwrap();
    assert_eq!(
        (rejected[0].status, rejected[0].decision_note.as_str()),
        (AbsenceStatus::Rejected, "busy week")
    );

    // In the personal workspace there is nothing to decide.
    let own = personal
        .save_absence_type(
            None,
            AbsenceTypeInput {
                name: "Leave".into(),
                color: "#111111".into(),
                reduces_target: true,
                affects_vacation: true,
            },
        )
        .await
        .unwrap();
    let a = personal
        .save_absence(
            None,
            AbsenceInput {
                type_id: own.id,
                start_date: date("2026-10-19"),
                end_date: date("2026-10-19"),
                half_day: None,
                note: String::new(),
            },
        )
        .await
        .unwrap();
    assert_eq!(a.status, AbsenceStatus::Approved);
}

#[tokio::test]
async fn the_team_summary_adds_up_each_persons_projects() {
    use zeima_core::model::{
        ActivityInput, BudgetPeriod, EntryInput, ProjectInput, ProjectStatus, ProjectVisibility,
    };
    use zeima_core::summary::Range;
    const HOUR: i64 = 3_600_000;
    let (_, ada, ben) = setup().await;
    let activity = ada
        .create_activity(ActivityInput {
            name: "Work".into(),
            color: "#111111".into(),
            archived: false,
        })
        .await
        .unwrap();
    let project = ada
        .create_project(ProjectInput {
            name: "Site".into(),
            alias: None,
            color: "#3b82f6".into(),
            parent_id: None,
            client: None,
            budget_minutes: None,
            budget_period: BudgetPeriod::Total,
            budget_warn_pct: 80,
            billable_default: true,
            status: ProjectStatus::Active,
            cost_center_id: None,
            visibility: ProjectVisibility::Org,
            manager_id: None,
            member_ids: vec![],
        })
        .await
        .unwrap();
    let entry = |project: Option<&str>, start: i64, hours: i64| EntryInput {
        activity_id: Some(activity.id.clone()),
        project_id: project.map(Into::into),
        start_at: start,
        end_at: Some(start + hours * HOUR),
        tz: Some("UTC".into()),
        note: String::new(),
        billable: None,
        tag_ids: vec![],
        cost_center_id: None,
    };
    ada.create_entry(entry(Some(&project.id), T0, 3)).await.unwrap();
    ada.create_entry(entry(None, T0 + 4 * HOUR, 1)).await.unwrap();
    // Ben's entry starts before the range: only the part inside counts.
    ben.create_entry(entry(Some(&project.id), T0 - 2 * HOUR, 4))
        .await
        .unwrap();

    let team = ada
        .team_summary(Range {
            from: T0,
            to: T0 + 24 * HOUR,
        })
        .await
        .unwrap();
    assert_eq!(team.len(), 2);
    assert_eq!(
        (team[0].user_id.as_str(), team[0].total_ms, team[0].billable_ms),
        (ADA, 4 * HOUR, 3 * HOUR)
    );
    assert_eq!(
        team[0]
            .projects
            .iter()
            .map(|p| (p.project_id.as_deref(), p.ms))
            .collect::<Vec<_>>(),
        vec![(Some(project.id.as_str()), 3 * HOUR), (None, HOUR)]
    );
    assert_eq!((team[1].user_id.as_str(), team[1].total_ms), (BEN, 2 * HOUR));
}

#[tokio::test]
async fn work_locations_are_one_row_per_person_and_day() {
    use zeima_core::LocationKind;
    let (personal, ada, ben) = setup().await;
    ada.set_location(date("2026-10-05"), Some(LocationKind::Office), None)
        .await
        .unwrap();
    ada.set_location(date("2026-10-05"), Some(LocationKind::Home), None)
        .await
        .unwrap();
    ben.set_location(date("2026-10-05"), Some(LocationKind::Travel), None)
        .await
        .unwrap();
    let day = ada
        .list_locations(date("2026-10-05"), date("2026-10-05"))
        .await
        .unwrap();
    assert_eq!(
        day.iter()
            .map(|l| (l.user_id.as_str(), l.kind))
            .collect::<Vec<_>>(),
        vec![(ADA, LocationKind::Home), (BEN, LocationKind::Travel)]
    );
    // Clearing and setting again reuses the row (the same id on every device).
    let id = day[0].id.clone();
    ada.set_location(date("2026-10-05"), None, None).await.unwrap();
    assert_eq!(
        ada.list_locations(date("2026-10-05"), date("2026-10-05"))
            .await
            .unwrap()
            .len(),
        1
    );
    ada.set_location(date("2026-10-05"), Some(LocationKind::Office), None)
        .await
        .unwrap();
    let again = ada
        .list_locations(date("2026-10-05"), date("2026-10-05"))
        .await
        .unwrap();
    assert_eq!(again[0].id, id);
    assert!(personal
        .set_location(date("2026-10-05"), Some(LocationKind::Home), None)
        .await
        .is_err());
}

#[tokio::test]
async fn policies_apply_to_entries_in_a_shared_workspace_only() {
    use zeima_core::model::{ActivityInput, EntryInput};
    use zeima_core::policy::{ManagerVisibility, PolicySettings};
    const HOUR: i64 = 3_600_000;
    let (personal, ada, _) = setup().await;
    let activity = ada
        .create_activity(ActivityInput {
            name: "Work".into(),
            color: "#111111".into(),
            archived: false,
        })
        .await
        .unwrap();
    let entry = |project: Option<&str>, tags: Vec<String>| EntryInput {
        activity_id: Some(activity.id.clone()),
        project_id: project.map(Into::into),
        start_at: T0,
        end_at: Some(T0 + HOUR),
        tz: Some("UTC".into()),
        note: String::new(),
        billable: None,
        tag_ids: tags,
        cost_center_id: None,
    };
    // Nothing is required until a policy says so.
    ada.create_entry(entry(None, vec![])).await.unwrap();

    // The organization asks for a project; the "dev" department also for a tag.
    ada.save_policy(
        None,
        PolicySettings {
            require_project: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    ada.save_policy(
        Some("dev".into()),
        PolicySettings {
            require_tags: true,
            manager_visibility: ManagerVisibility::HideNotes,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(ada.list_policies().await.unwrap().len(), 2);
    let refused = ada.create_entry(entry(None, vec![])).await.unwrap_err();
    assert!(
        matches!(refused, Error::Validation { ref field, .. } if field == "projectId"),
        "{refused}"
    );

    // Ada is not in "dev" yet: the tag is not required of her.
    let project = ada
        .create_project(zeima_core::model::ProjectInput {
            name: "Site".into(),
            alias: None,
            color: "#3b82f6".into(),
            parent_id: None,
            client: None,
            budget_minutes: None,
            budget_period: zeima_core::model::BudgetPeriod::Total,
            budget_warn_pct: 80,
            billable_default: false,
            status: zeima_core::model::ProjectStatus::Active,
            cost_center_id: None,
            visibility: Default::default(),
            manager_id: None,
            member_ids: vec![],
        })
        .await
        .unwrap();
    ada.create_entry(entry(Some(&project.id), vec![])).await.unwrap();
    ada.set_meta(
        zeima_core::ACCOUNT_GROUPS_KEY,
        &format!("[\"/orgs/{ORG}/groups/dev\"]"),
    )
    .await
    .unwrap();
    assert_eq!(
        ada.effective_policy().await.unwrap().manager_visibility,
        ManagerVisibility::HideNotes
    );
    let refused = ada
        .create_entry(entry(Some(&project.id), vec![]))
        .await
        .unwrap_err();
    assert!(
        matches!(refused, Error::Validation { ref field, .. } if field == "tagIds"),
        "{refused}"
    );

    // Saving the same scope again replaces it; deleting removes it.
    let again = ada.save_policy(None, PolicySettings::default()).await.unwrap();
    assert_eq!(ada.list_policies().await.unwrap().len(), 2);
    ada.delete_policy(&again.id).await.unwrap();
    assert_eq!(ada.list_policies().await.unwrap().len(), 1);

    // The personal workspace has no policies.
    assert!(personal
        .save_policy(None, PolicySettings::default())
        .await
        .is_err());
    assert_eq!(
        personal.effective_policy().await.unwrap(),
        PolicySettings::default()
    );
}

#[tokio::test]
async fn the_team_summary_also_sums_tags_and_cost_centers_and_lists_a_persons_entries() {
    use zeima_core::model::{ActivityInput, EntryInput, TagInput};
    use zeima_core::summary::Range;
    const HOUR: i64 = 3_600_000;
    let (_, ada, ben) = setup().await;
    let activity = ada
        .create_activity(ActivityInput {
            name: "Work".into(),
            color: "#111111".into(),
            archived: false,
        })
        .await
        .unwrap();
    let tag = ada
        .create_tag(TagInput {
            name: "dev".into(),
            color: "#3b82f6".into(),
            project_id: None,
            group_name: None,
        })
        .await
        .unwrap();
    let entry = |hours: i64, tags: Vec<String>, note: &str| EntryInput {
        activity_id: Some(activity.id.clone()),
        project_id: None,
        start_at: T0,
        end_at: Some(T0 + hours * HOUR),
        tz: Some("UTC".into()),
        note: note.into(),
        billable: None,
        tag_ids: tags,
        cost_center_id: None,
    };
    ada.create_entry(entry(2, vec![tag.id.clone()], "coding"))
        .await
        .unwrap();
    ada.create_entry(entry(1, vec![], "plain")).await.unwrap();
    let range = Range {
        from: T0,
        to: T0 + 24 * HOUR,
    };
    let team = ben.team_summary(range).await.unwrap();
    assert_eq!(team.len(), 1);
    assert_eq!(
        team[0]
            .tags
            .iter()
            .map(|t| (t.id.as_str(), t.ms))
            .collect::<Vec<_>>(),
        vec![(tag.id.as_str(), 2 * HOUR)]
    );
    assert!(team[0].cost_centers.is_empty());
    let entries = ben.team_entries(ADA, range).await.unwrap();
    assert_eq!(
        entries.iter().map(|e| e.note.as_str()).collect::<Vec<_>>().len(),
        2
    );
    assert!(ben.team_entries(BEN, range).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_day_in_the_office_can_name_the_office() {
    use zeima_core::LocationKind;
    let (_, ada, _) = setup().await;
    let vienna = ada.save_office(None, " Vienna ", Some(20)).await.unwrap();
    assert_eq!((vienna.name.as_str(), vienna.capacity), ("Vienna", Some(20)));
    assert!(ada.save_office(None, "", None).await.is_err());
    assert!(ada.save_office(None, "Tiny", Some(0)).await.is_err());
    let renamed = ada
        .save_office(Some(&vienna.id), "Vienna HQ", None)
        .await
        .unwrap();
    assert_eq!(
        (renamed.id.as_str(), renamed.capacity),
        (vienna.id.as_str(), None)
    );

    ada.set_location(
        date("2026-10-05"),
        Some(LocationKind::Office),
        Some(vienna.id.clone()),
    )
    .await
    .unwrap();
    let day = ada
        .list_locations(date("2026-10-05"), date("2026-10-05"))
        .await
        .unwrap();
    assert_eq!(day[0].office_id.as_deref(), Some(vienna.id.as_str()));
    // Only office days have an office, and it must exist.
    ada.set_location(
        date("2026-10-06"),
        Some(LocationKind::Home),
        Some(vienna.id.clone()),
    )
    .await
    .unwrap();
    assert_eq!(
        ada.list_locations(date("2026-10-06"), date("2026-10-06"))
            .await
            .unwrap()[0]
            .office_id,
        None
    );
    assert!(ada
        .set_location(
            date("2026-10-07"),
            Some(LocationKind::Office),
            Some("nope".into())
        )
        .await
        .is_err());
    ada.delete_office(&vienna.id).await.unwrap();
    assert!(ada.list_offices().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_usual_week_fills_the_empty_days() {
    use zeima_core::{LocationKind, LocationPattern, PatternDay};
    let (_, ada, _) = setup().await;
    let day = |weekday, kind| PatternDay {
        weekday,
        kind,
        office_id: None,
    };
    assert!(ada
        .set_location_pattern(LocationPattern {
            days: vec![day(8, LocationKind::Home)]
        })
        .await
        .is_err());
    ada.set_location_pattern(LocationPattern {
        days: vec![
            day(1, LocationKind::Office),
            day(3, LocationKind::Office),
            day(5, LocationKind::Home),
        ],
    })
    .await
    .unwrap();
    // 5 to 11 October 2026 is a week from Monday; Wednesday is already set.
    ada.set_location(date("2026-10-07"), Some(LocationKind::Travel), None)
        .await
        .unwrap();
    assert_eq!(
        ada.apply_location_pattern(date("2026-10-05"), date("2026-10-11"))
            .await
            .unwrap(),
        2
    );
    let week = ada
        .list_locations(date("2026-10-05"), date("2026-10-11"))
        .await
        .unwrap();
    assert_eq!(
        week.iter()
            .map(|l| (l.date.to_string(), l.kind))
            .collect::<Vec<_>>(),
        vec![
            ("2026-10-05".to_string(), LocationKind::Office),
            ("2026-10-07".to_string(), LocationKind::Travel),
            ("2026-10-09".to_string(), LocationKind::Home),
        ]
    );
    // Applying again changes nothing.
    assert_eq!(
        ada.apply_location_pattern(date("2026-10-05"), date("2026-10-11"))
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn meetings_are_called_answered_and_repeat() {
    use zeima_core::{MeetingInput, RsvpStatus};
    const HOUR: i64 = 3_600_000;
    const T_MON: i64 = 1_791_187_200_000; // 2026-10-05 08:00 UTC, a Monday
    let (_, ada, ben) = setup().await;
    ada.set_default_tz("UTC").unwrap();
    let input = |rrule: Option<&str>, attendees: Vec<&str>| MeetingInput {
        title: " Weekly sync ".into(),
        start_at: T_MON,
        end_at: T_MON + HOUR,
        tz: None,
        location: Some("Room 4".into()),
        online_url: Some("https://meet.example.com/x".into()),
        project_id: None,
        note: "agenda".into(),
        rrule: rrule.map(Into::into),
        attendee_ids: attendees.into_iter().map(Into::into).collect(),
    };
    // Checks.
    assert!(ada
        .create_meeting(MeetingInput {
            end_at: T_MON,
            ..input(None, vec![])
        })
        .await
        .is_err());
    assert!(ada
        .create_meeting(input(Some("FREQ=HOURLY"), vec![]))
        .await
        .is_err());
    assert!(ada
        .create_meeting(MeetingInput {
            online_url: Some("ftp://x".into()),
            ..input(None, vec![])
        })
        .await
        .is_err());

    let meeting = ada
        .create_meeting(input(Some("FREQ=WEEKLY;COUNT=3"), vec![BEN, ADA, BEN]))
        .await
        .unwrap();
    assert_eq!(
        (meeting.title.as_str(), meeting.attendee_ids.clone()),
        ("Weekly sync", vec![BEN.to_string()]),
        "the organizer is not an attendee, and none twice"
    );

    // Three weeks of occurrences, for the organizer and the person asked.
    let from = date("2026-10-01");
    let to = date("2026-11-30");
    assert_eq!(ada.list_meetings(from, to).await.unwrap().len(), 3);
    assert_eq!(ben.list_meetings(from, to).await.unwrap().len(), 3);
    assert_eq!(
        ada.list_meetings(date("2026-10-12"), date("2026-10-12"))
            .await
            .unwrap()
            .len(),
        1
    );

    // Only those asked answer; the organizer cannot.
    assert!(ada
        .respond_to_meeting(&meeting.id, RsvpStatus::Accepted)
        .await
        .is_err());
    ben.respond_to_meeting(&meeting.id, RsvpStatus::Tentative)
        .await
        .unwrap();
    ben.respond_to_meeting(&meeting.id, RsvpStatus::Accepted)
        .await
        .unwrap();
    let seen = ada.list_meetings(from, from).await.unwrap_or_default();
    assert!(seen.is_empty());
    let first = &ada.list_meetings(from, to).await.unwrap()[0];
    assert_eq!(
        (first.responses.len(), first.responses[0].status, first.my_status),
        (1, RsvpStatus::Accepted, None)
    );
    assert_eq!(
        ben.list_meetings(from, to).await.unwrap()[0].my_status,
        Some(RsvpStatus::Accepted)
    );

    // Only the organizer changes or cancels.
    assert!(ben
        .update_meeting(&meeting.id, input(None, vec![]))
        .await
        .is_err());
    assert!(ben.cancel_meeting(&meeting.id).await.is_err());
    let moved = ada
        .update_meeting(
            &meeting.id,
            MeetingInput {
                start_at: T_MON + 2 * HOUR,
                end_at: T_MON + 3 * HOUR,
                ..input(None, vec![BEN])
            },
        )
        .await
        .unwrap();
    assert_eq!(moved.start_at, T_MON + 2 * HOUR);
    assert_eq!(
        ada.list_meetings(from, to).await.unwrap().len(),
        1,
        "no repeat any more"
    );
    let called_off = ada.cancel_meeting(&meeting.id).await.unwrap();
    assert!(called_off.cancelled);
    assert!(ben.list_meetings(from, to).await.unwrap()[0].meeting.cancelled);

    let ics = ada.meeting_ics(&meeting.id).await.unwrap();
    assert!(
        ics.starts_with("BEGIN:VCALENDAR\r\n")
            && ics.contains("SUMMARY:Weekly sync")
            && ics.contains("LOCATION:Room 4")
            && ics.contains("STATUS:CANCELLED"),
        "{ics}"
    );
}
