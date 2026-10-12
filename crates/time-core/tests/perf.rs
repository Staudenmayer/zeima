#![cfg(feature = "db")]
//! One year of seeded data: the dashboard and reports must stay fast
//! (docs/SPEC.md §9: dashboard < 300 ms). Run with
//! `cargo test -p time-core --features db --release --test perf -- --ignored --nocapture`.

use std::sync::Arc;
use std::time::Instant;

use chrono::Duration;
use time_core::clock::ManualClock;
use time_core::model::{BudgetPeriod, EntryInput, ProjectInput, ProjectStatus};
use time_core::report::{ReportFilter, ReportGroup, ReportQuery};
use time_core::summary::Range;
use time_core::Core;

const HOUR: i64 = 3_600_000;
const DAY: i64 = 24 * HOUR;
/// 2026-10-05 00:00 UTC.
const NOW: i64 = 1_791_158_400_000;

#[tokio::test]
#[ignore = "performance check; run in release mode"]
async fn one_year_dashboard_is_fast() {
    let core = Core::open_in_memory(Arc::new(ManualClock::new(NOW)))
        .await
        .unwrap();
    core.set_default_tz("UTC").unwrap();
    let mut projects = Vec::new();
    for i in 0..10 {
        let p = core
            .create_project(ProjectInput {
                name: format!("Project {i}"),
                alias: None,
                color: "#3b82f6".into(),
                parent_id: None,
                client: None,
                budget_minutes: Some(10_000),
                budget_period: BudgetPeriod::Monthly,
                budget_warn_pct: 80,
                billable_default: i % 2 == 0,
                status: ProjectStatus::Active,
                cost_center_id: None,
            })
            .await
            .unwrap();
        projects.push(p.id);
    }
    // 365 days × 6 entries of 1 h 15 min from 08:00.
    let mut count = 0;
    for d in 0..365 {
        let day = NOW - (365 - d) * DAY;
        for k in 0..6 {
            let start = day + 8 * HOUR + k * (HOUR + HOUR / 4);
            core.create_entry(EntryInput {
                project_id: Some(projects[(d as usize + k as usize) % projects.len()].clone()),
                start_at: start,
                end_at: Some(start + HOUR + HOUR / 4),
                tz: None,
                note: String::new(),
                billable: None,
                tag_ids: vec![],
                cost_center_id: None,
            })
            .await
            .unwrap();
            count += 1;
        }
    }
    println!("seeded {count} entries");

    let today = core.today().unwrap();
    // Warm the daily summary cache, as a running app would have.
    core.work_days(today - Duration::days(365), today).await.unwrap();

    let started = Instant::now();
    let week = Range {
        from: NOW - 7 * DAY,
        to: NOW,
    };
    core.summaries(
        (0..7)
            .map(|i| Range {
                from: week.from + i * DAY,
                to: week.from + (i + 1) * DAY,
            })
            .collect(),
    )
    .await
    .unwrap();
    core.work_days(today - Duration::days(7), today).await.unwrap();
    core.balance().await.unwrap();
    core.budget_statuses().await.unwrap();
    core.report(&ReportQuery {
        range: Range {
            from: NOW - 30 * DAY,
            to: NOW,
        },
        filter: ReportFilter::default(),
        group_by: vec![ReportGroup::CostCenter],
    })
    .await
    .unwrap();
    let dashboard = started.elapsed();

    let started = Instant::now();
    let year = core
        .report(&ReportQuery {
            range: Range {
                from: NOW - 366 * DAY,
                to: NOW,
            },
            filter: ReportFilter::default(),
            group_by: vec![ReportGroup::Month, ReportGroup::Project],
        })
        .await
        .unwrap();
    let report = started.elapsed();

    println!(
        "dashboard: {dashboard:?}, one-year report: {report:?} ({} entries)",
        year.entry_count
    );
    assert!(dashboard.as_millis() < 300, "dashboard took {dashboard:?}");
}
