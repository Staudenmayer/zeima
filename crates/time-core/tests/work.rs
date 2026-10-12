#![cfg(feature = "db")]

use std::sync::Arc;

use chrono::{NaiveDate, TimeZone};
use time_core::clock::ManualClock;
use time_core::model::{AbsenceInput, BalanceAdjustmentInput, EntryInput, HalfDay};
use time_core::worktime::{parse_hhmm, ScheduleModel};
use time_core::{Core, Error, WorkScheduleInput};

const MIN: i64 = 60_000;
const HOUR: i64 = 60 * MIN;

fn date(s: &str) -> NaiveDate {
    s.parse().unwrap()
}

fn at(day: &str, hhmm: &str) -> i64 {
    let local = date(day).and_time(parse_hhmm(hhmm).unwrap());
    chrono_tz::Europe::Vienna
        .from_local_datetime(&local)
        .single()
        .unwrap()
        .timestamp_millis()
}

/// "Now" is Saturday 31 Oct 2026, 12:00 in Vienna; Austria selected.
async fn setup() -> (Core, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new(at("2026-10-31", "12:00")));
    let core = Core::open_in_memory(clock.clone()).await.unwrap();
    core.set_default_tz("Europe/Vienna").unwrap();
    core.set_jurisdiction("AT", Some("AT-9")).await.unwrap();
    // Default schedule starts "today"; move it to the start of the month.
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
    (core, clock)
}

async fn work(core: &Core, day: &str, from: &str, to: &str) -> String {
    core.create_entry(EntryInput {
        project_id: None,
        start_at: at(day, from),
        end_at: Some(at(day, to)),
        tz: None,
        note: String::new(),
        billable: None,
        tag_ids: vec![],
        cost_center_id: None,
    })
    .await
    .unwrap()
    .id
}

#[tokio::test]
async fn selecting_a_country_creates_a_default_schedule() {
    let clock = Arc::new(ManualClock::new(at("2026-10-31", "12:00")));
    let core = Core::open_in_memory(clock).await.unwrap();
    assert!(core.jurisdiction().await.unwrap().is_none());
    core.set_jurisdiction("DE", Some("DE-BY")).await.unwrap();
    let schedules = core.list_schedules().await.unwrap();
    assert_eq!(schedules.len(), 1);
    let targets: Vec<u32> = schedules[0].days.iter().map(|d| d.target_minutes).collect();
    assert_eq!(targets, vec![480, 480, 480, 480, 480, 0, 0]);
    let summary = core.profile_summary("de").await.unwrap();
    assert_eq!(summary.name, "Deutschland");
    assert_eq!(summary.region_name.as_deref(), Some("Bayern"));
    assert!(core.set_jurisdiction("XX", None).await.is_err());
}

#[tokio::test]
async fn work_days_follow_entries_holidays_and_breaks() {
    let (core, _) = setup().await;
    work(&core, "2026-10-26", "08:00", "12:00").await; // holiday (Nationalfeiertag)
    work(&core, "2026-10-27", "08:00", "15:00").await; // 7 h without break
    let days = core
        .work_days(date("2026-10-26"), date("2026-10-27"))
        .await
        .unwrap();
    assert_eq!(days[0].target_ms, 0);
    assert_eq!(days[0].delta_ms, 4 * HOUR);
    assert_eq!(days[0].holiday.as_deref(), Some("national_day"));
    assert_eq!(days[1].break_deducted_ms, 30 * MIN);
    assert_eq!(days[1].delta_ms, -90 * MIN);
}

async fn day(core: &Core) -> time_core::worktime::WorkDay {
    core.work_days(date("2026-10-20"), date("2026-10-20"))
        .await
        .unwrap()
        .remove(0)
}

#[tokio::test]
async fn cache_is_invalidated_by_every_kind_of_change() {
    let (core, _) = setup().await;
    let id = work(&core, "2026-10-20", "08:00", "16:00").await;
    // First read fills the cache; the second must come from it and be equal.
    let first = day(&core).await;
    assert_eq!(first, day(&core).await);
    assert_eq!(first.delta_ms, -30 * MIN, "8 h without break → 30 min deducted");

    // Entry edit
    core.update_entry(
        &id,
        EntryInput {
            project_id: None,
            start_at: at("2026-10-20", "08:00"),
            end_at: Some(at("2026-10-20", "17:00")),
            tz: None,
            note: String::new(),
            billable: None,
            tag_ids: vec![],
            cost_center_id: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        day(&core).await.delta_ms,
        9 * HOUR - 30 * MIN - 8 * HOUR,
        "9 h − 30 min break − 8 h target"
    );

    // Absence (half day)
    let vacation = core
        .list_absence_types()
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.key.as_deref() == Some("vacation"))
        .unwrap();
    let absence = core
        .save_absence(
            None,
            AbsenceInput {
                type_id: vacation.id.clone(),
                start_date: date("2026-10-20"),
                end_date: date("2026-10-20"),
                half_day: Some(HalfDay::Am),
                note: String::new(),
            },
        )
        .await
        .unwrap();
    assert_eq!(day(&core).await.target_ms, 4 * HOUR);
    core.delete_absence(&absence.id).await.unwrap();
    assert_eq!(day(&core).await.target_ms, 8 * HOUR);

    // Custom holiday
    let h = core
        .create_custom_holiday(date("2026-10-20"), "Company day")
        .await
        .unwrap();
    assert_eq!(day(&core).await.target_ms, 0);
    core.delete_custom_holiday(&h.id).await.unwrap();

    // Schedule change
    let s = core.list_schedules().await.unwrap().remove(0);
    let mut days = s.days.clone();
    days[1].target_minutes = 6 * 60; // Tuesday
    core.save_schedule(
        Some(&s.id),
        WorkScheduleInput {
            valid_from: s.valid_from,
            model: ScheduleModel::Flextime,
            days,
        },
    )
    .await
    .unwrap();
    assert_eq!(day(&core).await.target_ms, 6 * HOUR);

    // Settings: no break deduction
    let mut settings = core.work_settings().await.unwrap();
    settings.deduct_breaks = false;
    core.set_work_settings(settings).await.unwrap();
    assert_eq!(day(&core).await.break_deducted_ms, 0);

    // Entry delete
    core.delete_entry(&id).await.unwrap();
    assert_eq!(day(&core).await.worked_ms, 0);

    // Cached and freshly computed values agree.
    let cached = core
        .work_days(date("2026-10-01"), date("2026-10-31"))
        .await
        .unwrap();
    let fresh = core
        .compute_range(date("2026-10-01"), date("2026-10-31"))
        .await
        .unwrap();
    assert_eq!(cached, fresh);
}

#[tokio::test]
async fn balance_includes_adjustments_and_today() {
    let (core, clock) = setup().await;
    clock.set(at("2026-10-28", "12:00")); // Wednesday
                                          // October 1–27: 18 workdays without any work → −18 × 8 h,
                                          // minus nothing on 26 Oct (holiday).
    let balance = core.balance().await.unwrap();
    assert_eq!(balance.since, Some(date("2026-10-01")));
    assert_eq!(balance.until_yesterday_ms, -18 * 8 * HOUR);
    assert_eq!(balance.today_ms, -8 * HOUR);

    core.create_adjustment(BalanceAdjustmentInput {
        date: date("2026-10-27"),
        minutes: 120,
        reason: "Carry-over".into(),
    })
    .await
    .unwrap();
    work(&core, "2026-10-28", "08:00", "11:00").await;
    let balance = core.balance().await.unwrap();
    assert_eq!(balance.until_yesterday_ms, -18 * 8 * HOUR + 2 * HOUR);
    assert_eq!(balance.today_ms, -5 * HOUR);

    assert!(matches!(
        core.create_adjustment(BalanceAdjustmentInput {
            date: date("2026-10-27"),
            minutes: 10,
            reason: " ".into()
        })
        .await,
        Err(Error::Validation { .. })
    ));
}

#[tokio::test]
async fn vacation_counts_workdays_only() {
    let (core, _) = setup().await;
    let vacation = core
        .list_absence_types()
        .await
        .unwrap()
        .into_iter()
        .find(|t| t.key.as_deref() == Some("vacation"))
        .unwrap();
    let book = |from: &str, to: &str, half: Option<HalfDay>| AbsenceInput {
        type_id: vacation.id.clone(),
        start_date: date(from),
        end_date: date(to),
        half_day: half,
        note: String::new(),
    };
    // Fri 23 – Wed 28 Oct: Mon 26 is a holiday, weekend free → Fri, Tue, Wed = 3
    core.save_absence(None, book("2026-10-23", "2026-10-28", None))
        .await
        .unwrap();
    // Half day in December (planned)
    core.save_absence(None, book("2026-12-09", "2026-12-09", Some(HalfDay::Pm)))
        .await
        .unwrap();
    let v = core.vacation_summary(2026).await.unwrap();
    assert_eq!(v.entitlement_days, 25.0);
    assert_eq!(v.used_days, 3.0);
    assert_eq!(v.planned_days, 0.5);
    assert_eq!(v.remaining_days, 21.5);

    assert!(core
        .save_absence(None, book("2026-12-09", "2026-12-10", Some(HalfDay::Am)))
        .await
        .is_err());
    assert!(core
        .save_absence(None, book("2026-12-10", "2026-12-09", None))
        .await
        .is_err());
}

#[tokio::test]
async fn builtin_absence_types_cannot_be_deleted() {
    let (core, _) = setup().await;
    let t = core.list_absence_types().await.unwrap().remove(0);
    assert!(matches!(
        core.delete_absence_type(&t.id).await,
        Err(Error::Conflict(_))
    ));
}

#[tokio::test]
async fn holidays_list_profile_and_custom_days() {
    let (core, _) = setup().await;
    core.create_custom_holiday(date("2026-11-15"), "Leopold")
        .await
        .unwrap();
    let list = core.holidays(2026, "de").await.unwrap();
    assert!(list
        .iter()
        .any(|h| h.name == "Nationalfeiertag" && h.custom_id.is_none()));
    assert!(list.iter().any(|h| h.name == "Leopold" && h.custom_id.is_some()));
    assert!(list.windows(2).all(|w| w[0].date <= w[1].date));
}
