//! Fills a Zeima database with demo data to try every local feature:
//! projects (budgets, clients, cost centers, an archived one), tags, cost
//! centers, three weeks of entries, absences and a custom holiday. Meetings
//! live in an organization's workspace, so they are not seeded here. Close the app first.
//!
//!   cargo run -p zeima-core --features db --example seed -- <path to zeima.db>
//!
//! It only adds; running it twice adds the data twice.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{Datelike, Duration, NaiveDate, TimeZone, Weekday};
use chrono_tz::Tz;
use zeima_core::clock::SystemClock;
use zeima_core::model::{
    AbsenceInput, ActivityInput, BudgetPeriod, CostCenterInput, EntryInput, HalfDay, ProjectInput,
    ProjectStatus, TagInput,
};
use zeima_core::Core;

fn at(tz: Tz, day: NaiveDate, h: u32, m: u32) -> i64 {
    tz.from_local_datetime(&day.and_hms_opt(h, m, 0).unwrap())
        .earliest()
        .unwrap()
        .timestamp_millis()
}

#[tokio::main]
async fn main() {
    let path: PathBuf = std::env::args()
        .nth(1)
        .expect("usage: seed <path to zeima.db>")
        .into();
    let core = Core::open(&path, Arc::new(SystemClock)).await.unwrap();
    let tz: Tz = core.default_tz().parse().unwrap_or(chrono_tz::Europe::Berlin);
    let today = core.today().unwrap();

    let only_absences = std::env::args().nth(2).as_deref() == Some("absences");
    if !only_absences {
        // Activities: reuse what exists, add what is missing.
        let mut acts = core.list_activities(false).await.unwrap();
        for (name, color) in [
            ("Development", "#3b82f6"),
            ("Meeting", "#a855f7"),
            ("Review", "#f59e0b"),
            ("Planning", "#14b8a6"),
            ("Support", "#ef4444"),
            ("Documentation", "#64748b"),
            ("Admin", "#78716c"),
        ] {
            if !acts.iter().any(|a| a.name == name) {
                let a = core
                    .create_activity(ActivityInput {
                        name: name.into(),
                        color: color.into(),
                        archived: false,
                    })
                    .await
                    .unwrap();
                acts.push(a);
            }
        }
        let act = |n: &str| acts.iter().find(|a| a.name == n).map(|a| a.id.clone());

        // Cost centers.
        let mut cc = Vec::new();
        for (code, name) in [
            ("CC-100", "Engineering"),
            ("CC-200", "Operations"),
            ("CC-300", "Sales"),
        ] {
            cc.push(
                core.save_cost_center(
                    None,
                    CostCenterInput {
                        code: code.into(),
                        name: name.into(),
                        parent_id: None,
                        active: code != "CC-300",
                        valid_from: None,
                        valid_to: None,
                    },
                )
                .await
                .unwrap()
                .id,
            );
        }

        // Projects.
        let proj = |name: &str,
                    alias: &str,
                    color: &str,
                    client: Option<&str>,
                    budget: Option<i64>,
                    period,
                    billable,
                    status,
                    cc: Option<String>| {
            ProjectInput {
                name: name.into(),
                alias: Some(alias.into()),
                color: color.into(),
                parent_id: None,
                client: client.map(Into::into),
                budget_minutes: budget,
                budget_period: period,
                budget_warn_pct: 80,
                billable_default: billable,
                status,
                cost_center_id: cc,
                visibility: Default::default(),
                manager_id: None,
                member_ids: Vec::new(),
            }
        };
        let defs = vec![
            proj(
                "Atlas",
                "ATL",
                "#3b82f6",
                Some("Acme Corp"),
                Some(6000),
                BudgetPeriod::Monthly,
                true,
                ProjectStatus::Active,
                Some(cc[0].clone()),
            ),
            proj(
                "Customer Portal",
                "PRT",
                "#22c55e",
                Some("Globex"),
                Some(2400),
                BudgetPeriod::Weekly,
                true,
                ProjectStatus::Active,
                Some(cc[0].clone()),
            ),
            proj(
                "Internal Tools",
                "TLS",
                "#f59e0b",
                None,
                None,
                BudgetPeriod::Total,
                false,
                ProjectStatus::Active,
                Some(cc[1].clone()),
            ),
            proj(
                "Research",
                "RES",
                "#a855f7",
                None,
                Some(1200),
                BudgetPeriod::Total,
                false,
                ProjectStatus::Paused,
                None,
            ),
            proj(
                "Legacy Migration",
                "LEG",
                "#78716c",
                Some("Initech"),
                None,
                BudgetPeriod::Total,
                true,
                ProjectStatus::Archived,
                None,
            ),
        ];
        let mut pid = Vec::new();
        for d in defs {
            pid.push(core.create_project(d).await.unwrap().id);
        }

        // Tags.
        let mut tid = Vec::new();
        for (name, color, group) in [
            ("urgent", "#ef4444", Some("Priority")),
            ("bug", "#f97316", Some("Kind")),
            ("feature", "#22c55e", Some("Kind")),
            ("customer", "#3b82f6", None),
            ("onsite", "#a855f7", None),
        ] {
            tid.push(
                core.create_tag(TagInput {
                    name: name.into(),
                    color: color.into(),
                    project_id: None,
                    group_name: group.map(Into::into),
                })
                .await
                .unwrap()
                .id,
            );
        }

        // Three weeks of weekdays: a mix of activities, projects, tags, billable.
        #[allow(clippy::type_complexity)]
        let plan: [(&str, usize, u32, u32, u32, u32, &[usize], &str); 6] = [
            ("Planning", 0, 8, 30, 9, 30, &[], "Plan the day"),
            ("Development", 0, 9, 30, 12, 15, &[2], "Implementation"),
            ("Review", 1, 13, 0, 14, 0, &[1], "Code review"),
            ("Development", 1, 14, 0, 16, 30, &[2, 3], "Portal work"),
            ("Support", 2, 16, 30, 17, 15, &[0, 1], "Tickets"),
            ("Documentation", 2, 17, 15, 17, 45, &[], ""),
        ];
        let mut n = 0;
        for back in (0..21).rev() {
            let day = today - Duration::days(back);
            if matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
                continue;
            }
            // Today stops at the current time: only past entries.
            for (i, (a, p, sh, sm, eh, em, tags, note)) in plan.iter().enumerate() {
                if back == 0 && i > 2 {
                    break;
                }
                // Vary: every 4th day swap project; every 5th, a different tag.
                let p = (p + (day.ordinal() as usize).is_multiple_of(4) as usize) % 3;
                core.create_entry(EntryInput {
                    activity_id: act(a),
                    project_id: Some(pid[p].clone()),
                    start_at: at(tz, day, *sh, *sm),
                    end_at: Some(at(tz, day, *eh, *em)),
                    tz: Some(tz.name().into()),
                    note: (*note).into(),
                    billable: None,
                    tag_ids: tags.iter().map(|t| tid[*t].clone()).collect(),
                    cost_center_id: if i == 4 { Some(cc[1].clone()) } else { None },
                })
                .await
                .unwrap();
                n += 1;
            }
        }
        println!("{n} entries");
    }

    // Absences: past sick day, a half day, upcoming vacation.
    let types = core.list_absence_types().await.unwrap();
    let ty = |k: &str| {
        types
            .iter()
            .find(|t| t.name.to_lowercase().starts_with(k))
            .map(|t| t.id.clone())
    };
    let wd = |mut d: NaiveDate, back: bool| {
        while matches!(d.weekday(), Weekday::Sat | Weekday::Sun) {
            d += Duration::days(if back { -1 } else { 1 });
        }
        d
    };
    let sick = wd(today - Duration::days(25), true);
    let half = wd(today - Duration::days(10), true);
    let vac = wd(today + Duration::days(10), false);
    for (k, s, e, h, note) in [
        ("sick", sick, sick, None, "Flu"),
        ("vac", half, half, Some(HalfDay::Pm), "Appointment"),
        (
            "vac",
            vac,
            wd(vac + Duration::days(4), false),
            None,
            "Summer break",
        ),
        (
            "train",
            wd(today + Duration::days(25), false),
            wd(today + Duration::days(26), false),
            None,
            "Conference",
        ),
    ] {
        if let Some(type_id) = ty(k) {
            core.save_absence(
                None,
                AbsenceInput {
                    type_id,
                    start_date: s,
                    end_date: e,
                    half_day: h,
                    note: note.into(),
                },
            )
            .await
            .unwrap();
        }
    }

    core.create_custom_holiday(today + Duration::days(14), "Company day")
        .await
        .unwrap();
    println!("done");
}
