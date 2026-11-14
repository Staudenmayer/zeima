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
