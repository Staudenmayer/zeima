#![cfg(feature = "db")]

//! Work-time data between two devices. Every device creates some of it for
//! itself (the built-in absence types, the work settings, a schedule), so the
//! devices must recognize each other's rows instead of duplicating them.

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::NaiveDate;
use zeima_core::clock::ManualClock;
use zeima_core::model::{AbsenceInput, BalanceAdjustmentInput};
use zeima_core::sync::{merge, Actor, Outcome, RowState};
use zeima_core::worktime::ScheduleModel;
use zeima_core::{Core, RemoteRow, WorkScheduleInput, SYNC_BATCH};

#[derive(Default)]
struct Server {
    rows: BTreeMap<(String, String), RowState>,
}

impl Server {
    async fn push(&mut self, device: &Core) {
        loop {
            let outbox = device.sync_outbox(SYNC_BATCH).await.unwrap();
            if outbox.is_empty() {
                break;
            }
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
    }

    async fn pull(&self, device: &Core) -> usize {
        let rows: Vec<RemoteRow> = self
            .rows
            .iter()
            .map(|((table, id), state)| RemoteRow {
                workspace_id: String::new(),
                table: table.clone(),
                row_id: id.clone(),
                fields: state.fields.clone(),
                deleted: state.deleted.clone(),
            })
            .collect();
        let report = device.sync_apply(&rows).await.unwrap();
        report.conflicts
    }

    /// Both devices send, then both receive.
    async fn sync(&mut self, a: &Core, b: &Core) {
        self.push(a).await;
        self.push(b).await;
        assert_eq!(self.pull(a).await, 0, "device a had conflicts");
        assert_eq!(self.pull(b).await, 0, "device b had conflicts");
    }
}

fn date(s: &str) -> NaiveDate {
    s.parse().unwrap()
}

async fn device(start: i64) -> (Core, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new(start));
    let core = Core::open_in_memory(clock.clone()).await.unwrap();
    core.set_default_tz("Europe/Vienna").unwrap();
    (core, clock)
}

const T0: i64 = 1_793_000_000_000; // late Oct 2026

async fn type_id(core: &Core, key: &str) -> String {
    core.list_absence_types()
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.key.as_deref() == Some(key))
        .unwrap()
        .id
}

#[tokio::test]
async fn built_in_absence_types_are_one_set_not_two() {
    let (a, _) = device(T0).await;
    let (b, _) = device(T0).await;
    let mut server = Server::default();
    // Each device made its own five built-in types, with different ids.
    assert_eq!(a.list_absence_types().await.unwrap().len(), 5);
    assert_ne!(type_id(&a, "vacation").await, type_id(&b, "vacation").await);
    let (a_vacation, b_vacation) = (type_id(&a, "vacation").await, type_id(&b, "vacation").await);

    server.sync(&a, &b).await;

    assert_eq!(
        a.list_absence_types().await.unwrap().len(),
        5,
        "no duplicates on a"
    );
    assert_eq!(
        b.list_absence_types().await.unwrap().len(),
        5,
        "no duplicates on b"
    );
    // Local ids stay as they were.
    assert_eq!(type_id(&a, "vacation").await, a_vacation);
    assert_eq!(type_id(&b, "vacation").await, b_vacation);
    // The server holds one set.
    assert_eq!(server.rows.keys().filter(|(t, _)| t == "absence_type").count(), 5);
}

#[tokio::test]
async fn an_absence_points_at_the_other_devices_own_type() {
    let (a, _) = device(T0).await;
    let (b, _) = device(T0).await;
    a.set_jurisdiction("AT", Some("AT-9")).await.unwrap();
    b.set_jurisdiction("AT", Some("AT-9")).await.unwrap();
    let mut server = Server::default();
    server.sync(&a, &b).await;

    let vacation_a = type_id(&a, "vacation").await;
    a.save_absence(
        None,
        AbsenceInput {
            type_id: vacation_a,
            start_date: date("2026-12-24"),
            end_date: date("2026-12-24"),
            half_day: None,
            note: "Christmas".into(),
        },
    )
    .await
    .unwrap();
    server.sync(&a, &b).await;

    let seen = b
        .list_absences(date("2026-12-01"), date("2026-12-31"))
        .await
        .unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].note, "Christmas");
    assert_eq!(
        seen[0].type_id,
        type_id(&b, "vacation").await,
        "b's own vacation type, not a's"
    );

    // And back: an absence b changes still points at a's type on a.
    let mut changed = AbsenceInput {
        type_id: type_id(&b, "sick").await,
        start_date: date("2026-12-24"),
        end_date: date("2026-12-24"),
        half_day: None,
        note: "Actually ill".into(),
    };
    b.save_absence(Some(&seen[0].id), changed.clone()).await.unwrap();
    server.sync(&a, &b).await;
    let back = a
        .list_absences(date("2026-12-01"), date("2026-12-31"))
        .await
        .unwrap();
    assert_eq!(back[0].type_id, type_id(&a, "sick").await);
    assert_eq!(back[0].note, "Actually ill");

    // A delete travels too.
    changed.note.clear();
    a.delete_absence(&back[0].id).await.unwrap();
    server.sync(&a, &b).await;
    assert!(b
        .list_absences(date("2026-12-01"), date("2026-12-31"))
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn work_settings_are_one_record_whose_fields_merge() {
    let (a, clock_a) = device(T0).await;
    let (b, clock_b) = device(T0 + 5_000).await;
    let mut server = Server::default();

    // Both devices have set their own settings before ever syncing.
    let mut sa = a.work_settings().await.unwrap();
    sa.vacation_days_per_year = Some(30.0);
    a.set_work_settings(sa).await.unwrap();
    let mut sb = b.work_settings().await.unwrap();
    sb.deduct_breaks = false;
    b.set_work_settings(sb).await.unwrap();

    server.sync(&a, &b).await;
    // One record on the server, no unique-index trouble on either device.
    assert_eq!(
        server.rows.keys().filter(|(t, _)| t == "work_settings").count(),
        1
    );
    let (seen_a, seen_b) = (a.work_settings().await.unwrap(), b.work_settings().await.unwrap());
    assert_eq!(seen_a, seen_b, "both devices agree");

    // Edits to different fields while offline both survive.
    clock_a.advance(10_000);
    let mut s = a.work_settings().await.unwrap();
    s.vacation_days_per_year = Some(25.0);
    a.set_work_settings(s).await.unwrap();
    clock_b.advance(20_000);
    let mut s = b.work_settings().await.unwrap();
    s.deduct_breaks = true;
    b.set_work_settings(s).await.unwrap();
    server.sync(&a, &b).await;
    for device in [&a, &b] {
        let s = device.work_settings().await.unwrap();
        assert_eq!(s.vacation_days_per_year, Some(25.0));
        assert!(s.deduct_breaks);
    }
}

#[tokio::test]
async fn the_country_selection_and_schedule_are_shared() {
    let (a, clock_a) = device(T0).await;
    let (b, _) = device(T0).await;
    let mut server = Server::default();

    a.set_jurisdiction("AT", Some("AT-9")).await.unwrap();
    // The default schedule both devices would make starts the same day.
    b.set_jurisdiction("DE", Some("DE-BY")).await.unwrap();
    // a's choice is newer.
    clock_a.advance(60_000);
    a.set_jurisdiction("AT", Some("AT-3")).await.unwrap();

    server.sync(&a, &b).await;
    let chosen = b.jurisdiction().await.unwrap().unwrap();
    assert_eq!(
        (chosen.profile_id.as_str(), chosen.region.as_deref()),
        ("AT", Some("AT-3"))
    );
    assert_eq!(a.list_schedules().await.unwrap().len(), 1);
    assert_eq!(
        b.list_schedules().await.unwrap().len(),
        1,
        "same start date: one schedule"
    );

    // Editing the schedule on a reaches b.
    let schedule = a.list_schedules().await.unwrap().remove(0);
    let mut days = schedule.days.clone();
    days[0].target_minutes = 360;
    clock_a.advance(60_000);
    a.save_schedule(
        Some(&schedule.id),
        WorkScheduleInput {
            valid_from: schedule.valid_from,
            model: ScheduleModel::Flextime,
            days,
        },
    )
    .await
    .unwrap();
    server.sync(&a, &b).await;
    let on_b = b.list_schedules().await.unwrap().remove(0);
    assert_eq!(on_b.days[0].target_minutes, 360);
    assert_eq!(on_b.model, ScheduleModel::Flextime);
}

#[tokio::test]
async fn holidays_and_balance_corrections_travel() {
    let (a, _) = device(T0).await;
    let (b, _) = device(T0).await;
    let mut server = Server::default();

    a.create_custom_holiday(date("2026-12-24"), "Company day")
        .await
        .unwrap();
    a.create_adjustment(BalanceAdjustmentInput {
        date: date("2026-11-02"),
        minutes: 90,
        reason: "Carried over".into(),
    })
    .await
    .unwrap();
    server.sync(&a, &b).await;

    let holidays = b.list_custom_holidays().await.unwrap();
    assert_eq!(holidays.len(), 1);
    assert_eq!(holidays[0].name, "Company day");
    let adjustments = b.list_adjustments().await.unwrap();
    assert_eq!(
        (adjustments[0].minutes, adjustments[0].reason.as_str()),
        (90, "Carried over")
    );

    b.delete_custom_holiday(&holidays[0].id).await.unwrap();
    server.sync(&a, &b).await;
    assert!(a.list_custom_holidays().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_device_that_takes_the_accounts_data_gets_the_same_set() {
    let (a, _) = device(T0).await;
    let (b, _) = device(T0).await;
    let mut server = Server::default();
    a.set_jurisdiction("AT", Some("AT-9")).await.unwrap();
    let mut s = a.work_settings().await.unwrap();
    s.vacation_days_per_year = Some(28.0);
    a.set_work_settings(s).await.unwrap();
    server.push(&a).await;

    // b joins and chooses "use the account's data".
    b.sync_wipe_local().await.unwrap();
    assert_eq!(server.pull(&b).await, 0);
    assert_eq!(
        b.work_settings().await.unwrap().vacation_days_per_year,
        Some(28.0)
    );
    assert_eq!(b.jurisdiction().await.unwrap().unwrap().profile_id, "AT");
    assert_eq!(b.list_absence_types().await.unwrap().len(), 5);
    assert_eq!(b.list_schedules().await.unwrap().len(), 1);
}

#[tokio::test]
async fn an_absence_from_the_other_device_changes_the_days_already_computed() {
    let (a, _) = device(T0).await;
    let (b, _) = device(T0).await;
    a.set_jurisdiction("AT", Some("AT-9")).await.unwrap();
    b.set_jurisdiction("AT", Some("AT-9")).await.unwrap();
    // A schedule that covers the day, with the same start date on both devices.
    for core in [&a, &b] {
        let schedule = core.list_schedules().await.unwrap().remove(0);
        core.save_schedule(
            Some(&schedule.id),
            WorkScheduleInput {
                valid_from: date("2026-10-01"),
                model: schedule.model,
                days: schedule.days,
            },
        )
        .await
        .unwrap();
    }
    let mut server = Server::default();
    server.sync(&a, &b).await;

    // b computes a past day, which caches the result (today is the 26th).
    let day = date("2026-10-14"); // a Wednesday
    let before = b.work_days(day, day).await.unwrap();
    assert!(before[0].target_ms > 0 && before[0].absence_type_ids.is_empty());

    a.save_absence(
        None,
        AbsenceInput {
            type_id: type_id(&a, "vacation").await,
            start_date: day,
            end_date: day,
            half_day: None,
            note: String::new(),
        },
    )
    .await
    .unwrap();
    server.sync(&a, &b).await;

    // The cached result must not survive the new absence.
    let after = b.work_days(day, day).await.unwrap();
    assert_eq!(after[0].absence_type_ids, vec![type_id(&b, "vacation").await]);
    assert_eq!(after[0].target_ms, 0, "a vacation day has no target");
}

#[tokio::test]
async fn absence_types_sync_when_added_edited_and_removed() {
    use zeima_core::model::AbsenceTypeInput;
    let (a, clock_a) = device(T0).await;
    let (b, clock_b) = device(T0).await;
    let mut server = Server::default();
    server.sync(&a, &b).await;

    // A custom type made on a reaches b, with the same id.
    let conference = a
        .save_absence_type(
            None,
            AbsenceTypeInput {
                name: "Conference".into(),
                color: "#112233".into(),
                reduces_target: true,
                affects_vacation: false,
            },
        )
        .await
        .unwrap();
    server.sync(&a, &b).await;
    let on_b = b.list_absence_types().await.unwrap();
    assert_eq!(on_b.len(), 6);
    let seen = on_b.iter().find(|t| t.id == conference.id).expect("same id on b");
    assert_eq!(
        (seen.name.as_str(), seen.color.as_str(), seen.key.as_deref()),
        ("Conference", "#112233", None)
    );

    // Edits travel: a recolors the built-in "sick" type, b renames the custom one.
    clock_a.advance(10_000);
    let sick_a = a
        .list_absence_types()
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.key.as_deref() == Some("sick"))
        .unwrap();
    a.save_absence_type(
        Some(&sick_a.id),
        AbsenceTypeInput {
            name: sick_a.name.clone(),
            color: "#ff0000".into(),
            reduces_target: sick_a.reduces_target,
            affects_vacation: sick_a.affects_vacation,
        },
    )
    .await
    .unwrap();
    clock_b.advance(20_000);
    b.save_absence_type(
        Some(&conference.id),
        AbsenceTypeInput {
            name: "Training day".into(),
            color: "#112233".into(),
            reduces_target: true,
            affects_vacation: false,
        },
    )
    .await
    .unwrap();
    server.sync(&a, &b).await;
    let sick_b = b
        .list_absence_types()
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.key.as_deref() == Some("sick"))
        .unwrap();
    assert_eq!(
        sick_b.color, "#ff0000",
        "a built-in type's edit reaches the other device's own row"
    );
    let renamed = a
        .list_absence_types()
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.id == conference.id)
        .unwrap();
    assert_eq!(renamed.name, "Training day");

    // Removing a custom type removes it everywhere.
    a.delete_absence_type(&conference.id).await.unwrap();
    server.sync(&a, &b).await;
    assert_eq!(b.list_absence_types().await.unwrap().len(), 5);
}
