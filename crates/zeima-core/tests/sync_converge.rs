#![cfg(feature = "db")]

//! Deterministic multi-device convergence. Several devices with skewed clocks
//! make random edits (some to the same project and the same field), delete
//! things and sync at random moments against a stand-in server. When they
//! have all synced at the end, every device must hold exactly the same
//! projects. The seed makes any failure reproducible:
//! `CONVERGE_SEED=17 cargo test -p zeima-core --features db --test sync_converge`.

use std::collections::BTreeMap;
use std::sync::Arc;

use zeima_core::clock::ManualClock;
use zeima_core::model::{BudgetPeriod, ProjectInput, ProjectStatus};
use zeima_core::sync::{merge, Actor, Outcome, RowState};
use zeima_core::{Core, RemoteRow, SYNC_BATCH};

/// xorshift64*: small, seedable, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

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

    async fn pull(&self, device: &Core) {
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
        assert_eq!(report.conflicts, 0, "no unique constraints are in play");
    }
}

fn input(name: &str, color: &str) -> ProjectInput {
    ProjectInput {
        name: name.into(),
        alias: None,
        color: color.into(),
        parent_id: None,
        client: None,
        budget_minutes: None,
        budget_period: BudgetPeriod::Total,
        budget_warn_pct: 80,
        billable_default: false,
        status: ProjectStatus::Active,
        cost_center_id: None,
        visibility: Default::default(),
        manager_id: None,
        member_ids: Vec::new(),
    }
}

/// What a device holds, in a comparable form.
async fn view(core: &Core) -> Vec<(String, String, String)> {
    let mut rows: Vec<_> = core
        .list_projects(true)
        .await
        .unwrap()
        .into_iter()
        .map(|p| (p.id, p.name, p.color))
        .collect();
    rows.sort();
    rows
}

async fn run(seed: u64, devices: usize, steps: usize) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut cores = Vec::new();
    let mut clocks = Vec::new();
    for i in 0..devices {
        // Skewed starting clocks, up to a minute apart.
        let clock = Arc::new(ManualClock::new(1_700_000_000_000 + (i as i64) * 20_000));
        cores.push(Core::open_in_memory(clock.clone()).await.unwrap());
        clocks.push(clock);
    }
    let mut server = Server::default();

    for step in 0..steps {
        let d = rng.below(devices);
        clocks[d].advance(1 + rng.below(5_000) as i64);
        match rng.below(10) {
            0..=2 => {
                let name = format!("p{seed}-{step}");
                cores[d].create_project(input(&name, "#000000")).await.unwrap();
            }
            3..=5 => {
                let known = cores[d].list_projects(true).await.unwrap();
                if !known.is_empty() {
                    let p = &known[rng.below(known.len())];
                    // Change only the name, or only the color, so two devices
                    // can edit different fields of one project.
                    let next = if rng.below(2) == 0 {
                        input(&format!("n{step}"), &p.color)
                    } else {
                        input(&p.name, &format!("#{:06x}", rng.below(0xFF_FFFF)))
                    };
                    cores[d].update_project(&p.id, next).await.unwrap();
                }
            }
            6 => {
                let known = cores[d].list_projects(true).await.unwrap();
                if !known.is_empty() {
                    let p = &known[rng.below(known.len())];
                    cores[d].delete_project(&p.id).await.unwrap();
                }
            }
            7 => server.push(&cores[d]).await,
            _ => {
                server.push(&cores[d]).await;
                server.pull(&cores[d]).await;
            }
        }
    }

    // Everyone sends what is left, then everyone catches up (twice, to show
    // that catching up changes nothing further).
    for core in &cores {
        server.push(core).await;
    }
    for _ in 0..2 {
        for core in &cores {
            server.pull(core).await;
        }
    }
    let first = view(&cores[0]).await;
    for (i, core) in cores.iter().enumerate().skip(1) {
        assert_eq!(
            view(core).await,
            first,
            "seed {seed}: device {i} differs from device 0"
        );
    }
    // Nothing is left to send after catching up.
    for core in &cores {
        assert!(
            core.sync_outbox(SYNC_BATCH).await.unwrap().is_empty(),
            "seed {seed}"
        );
    }
}

#[tokio::test]
async fn devices_converge_for_many_random_histories() {
    if let Ok(seed) = std::env::var("CONVERGE_SEED") {
        run(seed.parse().unwrap(), 3, 80).await;
        return;
    }
    // CONVERGE_SEEDS=600 runs a longer sweep.
    let seeds: u64 = std::env::var("CONVERGE_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(40);
    for seed in 1..=seeds {
        run(seed, 3, 80).await;
    }
}

#[tokio::test]
async fn two_devices_with_wildly_different_clocks_converge() {
    for seed in 100..110 {
        run(seed, 2, 60).await;
    }
}

// ---------------------------------------------------------------- work time

use chrono::NaiveDate;
use zeima_core::model::{AbsenceInput, BalanceAdjustmentInput};

fn day(n: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 12, 1 + n % 28).unwrap()
}

/// What a device holds of the work-time data, independent of local ids.
async fn work_view(core: &Core) -> String {
    let keys: std::collections::HashMap<String, String> = core
        .list_absence_types()
        .await
        .unwrap()
        .into_iter()
        .map(|t| (t.id, t.key.unwrap_or(t.name)))
        .collect();
    let mut absences: Vec<String> = core
        .list_absences(day(0), NaiveDate::from_ymd_opt(2026, 12, 31).unwrap())
        .await
        .unwrap()
        .into_iter()
        .map(|a| {
            format!(
                "{} {} {}..{} {:?} {}",
                a.id, keys[&a.type_id], a.start_date, a.end_date, a.half_day, a.note
            )
        })
        .collect();
    absences.sort();
    let mut holidays: Vec<String> = core
        .list_custom_holidays()
        .await
        .unwrap()
        .into_iter()
        .map(|h| format!("{} {} {}", h.id, h.date, h.name))
        .collect();
    holidays.sort();
    let mut adjustments: Vec<String> = core
        .list_adjustments()
        .await
        .unwrap()
        .into_iter()
        .map(|a| format!("{} {} {} {}", a.id, a.date, a.minutes, a.reason))
        .collect();
    adjustments.sort();
    let selection = core
        .jurisdiction()
        .await
        .unwrap()
        .map(|j| (j.profile_id, j.region));
    let mut types: Vec<_> = keys.values().cloned().collect();
    types.sort();
    format!(
        "settings={:?}\nselection={selection:?}\ntypes={types:?}\nabsences={absences:#?}\nholidays={holidays:#?}\nadjustments={adjustments:#?}",
        core.work_settings().await.unwrap()
    )
}

async fn run_work(seed: u64, devices: usize, steps: usize) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut cores = Vec::new();
    let mut clocks = Vec::new();
    for i in 0..devices {
        let clock = Arc::new(ManualClock::new(1_793_000_000_000 + (i as i64) * 20_000));
        let core = Core::open_in_memory(clock.clone()).await.unwrap();
        core.set_default_tz("Europe/Vienna").unwrap();
        cores.push(core);
        clocks.push(clock);
    }
    let mut server = Server::default();
    let profiles = [("AT", "AT-9"), ("AT", "AT-3"), ("DE", "DE-BY"), ("CH", "CH-ZH")];

    for step in 0..steps {
        let d = rng.below(devices);
        clocks[d].advance(1 + rng.below(5_000) as i64);
        match rng.below(12) {
            0 => {
                let (p, r) = profiles[rng.below(profiles.len())];
                cores[d].set_jurisdiction(p, Some(r)).await.unwrap();
            }
            1 | 2 => {
                let mut s = cores[d].work_settings().await.unwrap();
                if rng.below(2) == 0 {
                    s.vacation_days_per_year = Some(20.0 + rng.below(15) as f64);
                } else {
                    s.deduct_breaks = rng.below(2) == 0;
                }
                cores[d].set_work_settings(s).await.unwrap();
            }
            3 | 4 => {
                let types = cores[d].list_absence_types().await.unwrap();
                let t = &types[rng.below(types.len())];
                let start = day(rng.below(28) as u32);
                cores[d]
                    .save_absence(
                        None,
                        AbsenceInput {
                            type_id: t.id.clone(),
                            start_date: start,
                            end_date: start,
                            half_day: None,
                            note: format!("n{step}"),
                        },
                    )
                    .await
                    .unwrap();
            }
            5 => {
                let known = cores[d]
                    .list_absences(day(0), NaiveDate::from_ymd_opt(2026, 12, 31).unwrap())
                    .await
                    .unwrap();
                if !known.is_empty() {
                    let a = &known[rng.below(known.len())];
                    let types = cores[d].list_absence_types().await.unwrap();
                    let t = &types[rng.below(types.len())];
                    cores[d]
                        .save_absence(
                            Some(&a.id),
                            AbsenceInput {
                                type_id: t.id.clone(),
                                start_date: a.start_date,
                                end_date: a.end_date,
                                half_day: None,
                                note: format!("e{step}"),
                            },
                        )
                        .await
                        .unwrap();
                }
            }
            6 => {
                let known = cores[d]
                    .list_absences(day(0), NaiveDate::from_ymd_opt(2026, 12, 31).unwrap())
                    .await
                    .unwrap();
                if !known.is_empty() {
                    cores[d]
                        .delete_absence(&known[rng.below(known.len())].id)
                        .await
                        .unwrap();
                }
            }
            7 => {
                cores[d]
                    .create_custom_holiday(day(rng.below(28) as u32), &format!("h{step}"))
                    .await
                    .unwrap();
            }
            8 => {
                cores[d]
                    .create_adjustment(BalanceAdjustmentInput {
                        date: day(rng.below(28) as u32),
                        minutes: 1 + rng.below(120) as i64,
                        reason: format!("r{step}"),
                    })
                    .await
                    .unwrap();
            }
            9 => server.push(&cores[d]).await,
            _ => {
                server.push(&cores[d]).await;
                server.pull(&cores[d]).await;
            }
        }
    }

    for core in &cores {
        server.push(core).await;
    }
    for _ in 0..2 {
        for core in &cores {
            server.pull(core).await;
        }
    }
    let first = work_view(&cores[0]).await;
    for (i, core) in cores.iter().enumerate().skip(1) {
        assert_eq!(
            work_view(core).await,
            first,
            "seed {seed}: device {i} differs from device 0"
        );
    }
}

#[tokio::test]
async fn work_time_data_converges_for_many_random_histories() {
    let seeds: u64 = std::env::var("CONVERGE_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    for seed in 1..=seeds {
        run_work(seed, 3, 70).await;
    }
}
