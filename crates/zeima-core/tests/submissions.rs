#![cfg(feature = "db")]

mod common;

use std::sync::Arc;

use zeima_core::clock::ManualClock;
use zeima_core::model::BalanceAdjustmentInput;
use zeima_core::{Core, Error, SubmissionStatus, SYNC_BATCH};

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
async fn a_timesheet_is_submitted_then_decided_by_someone_else() {
    let (_, ada, ben) = setup().await;
    let sub = ada
        .submit_timesheet(date("2026-10-05"), date("2026-10-11"), " week 41 ")
        .await
        .unwrap();
    assert_eq!(
        (sub.status, sub.note.as_str(), sub.user_id.as_str()),
        (SubmissionStatus::Submitted, "week 41", ADA)
    );

    // Not twice, not overlapping.
    assert!(matches!(
        ada.submit_timesheet(date("2026-10-05"), date("2026-10-11"), "")
            .await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        ada.submit_timesheet(date("2026-10-10"), date("2026-10-18"), "")
            .await,
        Err(Error::Conflict(_))
    ));
    // Not before it starts.
    assert!(ada
        .submit_timesheet(date("2026-10-12"), date("2026-10-11"), "")
        .await
        .is_err());

    // Not by oneself.
    assert!(matches!(
        ada.decide_submission(&sub.id, true, "").await,
        Err(Error::Validation { .. })
    ));
    let approved = ben.decide_submission(&sub.id, true, "ok").await.unwrap();
    assert_eq!(
        (
            approved.status,
            approved.decided_by.as_deref(),
            approved.decision_note.as_str()
        ),
        (SubmissionStatus::Approved, Some(BEN), "ok")
    );
    assert!(approved.decided_at.is_some());
    // Decided once; no taking back an approved one.
    assert!(matches!(
        ben.decide_submission(&sub.id, false, "").await,
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        ada.withdraw_submission(&sub.id).await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(
        ada.list_submissions().await.unwrap()[0].status,
        SubmissionStatus::Approved
    );
}

#[tokio::test]
async fn a_rejected_timesheet_is_submitted_again_and_a_waiting_one_taken_back() {
    let (_, ada, ben) = setup().await;
    let first = ada
        .submit_timesheet(date("2026-10-12"), date("2026-10-18"), "")
        .await
        .unwrap();
    ben.decide_submission(&first.id, false, "hours missing")
        .await
        .unwrap();
    let again = ada
        .submit_timesheet(date("2026-10-12"), date("2026-10-18"), "fixed")
        .await
        .unwrap();
    assert_eq!(again.id, first.id, "the same record is submitted again");
    assert_eq!(
        (again.status, again.decided_by, again.decision_note.as_str()),
        (SubmissionStatus::Submitted, None, "")
    );
    ada.withdraw_submission(&again.id).await.unwrap();
    assert!(ada.list_submissions().await.unwrap().is_empty());
}

#[tokio::test]
async fn only_shared_workspaces_have_timesheets() {
    let (personal, ..) = setup().await;
    assert!(matches!(
        personal
            .submit_timesheet(date("2026-10-05"), date("2026-10-11"), "")
            .await,
        Err(Error::Validation { .. })
    ));
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
async fn an_approved_timesheet_can_be_reopened_by_someone_else() {
    let (_, ada, ben) = setup().await;
    let sub = ada
        .submit_timesheet(date("2026-10-05"), date("2026-10-11"), "")
        .await
        .unwrap();
    assert!(
        matches!(ben.reopen_submission(&sub.id, "").await, Err(Error::Conflict(_))),
        "only approved ones"
    );
    ben.decide_submission(&sub.id, true, "").await.unwrap();
    assert!(
        matches!(
            ada.reopen_submission(&sub.id, "").await,
            Err(Error::Validation { .. })
        ),
        "not one's own"
    );
    let reopened = ben.reopen_submission(&sub.id, "a day is missing").await.unwrap();
    assert_eq!(
        (reopened.status, reopened.decision_note.as_str()),
        (SubmissionStatus::Rejected, "a day is missing")
    );
    // The person can submit it again.
    assert_eq!(
        ada.submit_timesheet(date("2026-10-05"), date("2026-10-11"), "")
            .await
            .unwrap()
            .status,
        SubmissionStatus::Submitted
    );
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
    ada.set_location(date("2026-10-05"), Some(LocationKind::Office))
        .await
        .unwrap();
    ada.set_location(date("2026-10-05"), Some(LocationKind::Home))
        .await
        .unwrap();
    ben.set_location(date("2026-10-05"), Some(LocationKind::Travel))
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
    ada.set_location(date("2026-10-05"), None).await.unwrap();
    assert_eq!(
        ada.list_locations(date("2026-10-05"), date("2026-10-05"))
            .await
            .unwrap()
            .len(),
        1
    );
    ada.set_location(date("2026-10-05"), Some(LocationKind::Office))
        .await
        .unwrap();
    let again = ada
        .list_locations(date("2026-10-05"), date("2026-10-05"))
        .await
        .unwrap();
    assert_eq!(again[0].id, id);
    assert!(personal
        .set_location(date("2026-10-05"), Some(LocationKind::Home))
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
