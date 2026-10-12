#![cfg(feature = "db")]

mod common;

use std::sync::Arc;

use chrono::NaiveDate;
use zeima_core::clock::ManualClock;
use zeima_core::model::{
    BudgetPeriod, CostCenterAssignmentInput, CostCenterInput, EntryInput, ProjectInput, ProjectStatus,
    StartTimer, TimerState,
};
use zeima_core::{Core, Error};

const HOUR: i64 = 3_600_000;
/// 2026-10-05 08:00 UTC (a Monday).
const T0: i64 = 1_791_187_200_000;

async fn setup() -> Core {
    let core = common::with_activity(
        Core::open_in_memory(Arc::new(ManualClock::new(T0)))
            .await
            .unwrap(),
    )
    .await;
    core.set_default_tz("UTC").unwrap();
    core
}

fn date(s: &str) -> NaiveDate {
    s.parse().unwrap()
}

fn cc(code: &str) -> CostCenterInput {
    CostCenterInput {
        code: code.into(),
        name: format!("Cost center {code}"),
        parent_id: None,
        active: true,
        valid_from: None,
        valid_to: None,
    }
}

fn project(cost_center_id: Option<&str>) -> ProjectInput {
    ProjectInput {
        name: "Website".into(),
        alias: None,
        color: "#3b82f6".into(),
        parent_id: None,
        client: None,
        budget_minutes: None,
        budget_period: BudgetPeriod::Total,
        budget_warn_pct: 80,
        billable_default: false,
        status: ProjectStatus::Active,
        cost_center_id: cost_center_id.map(Into::into),
    }
}

fn entry(project_id: Option<&str>, start: i64, cost_center_id: Option<&str>) -> EntryInput {
    EntryInput {
        activity_id: common::activity(),
        project_id: project_id.map(Into::into),
        start_at: start,
        end_at: Some(start + HOUR),
        tz: None,
        note: String::new(),
        billable: None,
        tag_ids: vec![],
        cost_center_id: cost_center_id.map(Into::into),
    }
}

#[tokio::test]
async fn entry_cost_center_resolves_explicit_then_project_then_user() {
    let core = setup().await;
    let a = core.save_cost_center(None, cc("100")).await.unwrap();
    let b = core.save_cost_center(None, cc("200")).await.unwrap();
    let c = core.save_cost_center(None, cc("300")).await.unwrap();
    core.save_cost_center_assignment(
        None,
        CostCenterAssignmentInput {
            cost_center_id: c.id.clone(),
            valid_from: date("2026-01-01"),
            valid_to: None,
        },
    )
    .await
    .unwrap();
    let with_default = core.create_project(project(Some(&a.id))).await.unwrap();
    let without_default = core.create_project(project(None)).await.unwrap();

    let explicit = core
        .create_entry(entry(Some(&with_default.id), T0, Some(&b.id)))
        .await
        .unwrap();
    assert_eq!(explicit.cost_center_id.as_deref(), Some(b.id.as_str()));

    let from_project = core
        .create_entry(entry(Some(&with_default.id), T0, None))
        .await
        .unwrap();
    assert_eq!(from_project.cost_center_id.as_deref(), Some(a.id.as_str()));

    let from_user = core
        .create_entry(entry(Some(&without_default.id), T0, None))
        .await
        .unwrap();
    assert_eq!(from_user.cost_center_id.as_deref(), Some(c.id.as_str()));

    // Before the user's assignment starts there is no default.
    let before = core
        .create_entry(entry(None, T0 - 365 * 24 * HOUR, None))
        .await
        .unwrap();
    assert_eq!(before.cost_center_id, None);

    // Moving the entry to the other project re-resolves it.
    let moved = core
        .update_entry(&from_project.id, entry(Some(&without_default.id), T0, None))
        .await
        .unwrap();
    assert_eq!(moved.cost_center_id.as_deref(), Some(c.id.as_str()));

    // The timer resolves too.
    let TimerState::Running { entry } = core
        .start_timer(StartTimer {
            activity_id: common::activity(),
            project_id: Some(with_default.id.clone()),
            tag_ids: vec![],
            note: String::new(),
            billable: None,
        })
        .await
        .unwrap()
    else {
        panic!("timer not running");
    };
    assert_eq!(entry.cost_center_id.as_deref(), Some(a.id.as_str()));
}

#[tokio::test]
async fn inactive_or_expired_defaults_are_skipped() {
    let core = setup().await;
    let expired = core
        .save_cost_center(
            None,
            CostCenterInput {
                valid_to: Some(date("2025-12-31")),
                ..cc("100")
            },
        )
        .await
        .unwrap();
    let user_default = core.save_cost_center(None, cc("200")).await.unwrap();
    core.save_cost_center_assignment(
        None,
        CostCenterAssignmentInput {
            cost_center_id: user_default.id.clone(),
            valid_from: date("2026-01-01"),
            valid_to: None,
        },
    )
    .await
    .unwrap();
    let p = core.create_project(project(Some(&expired.id))).await.unwrap();
    let e = core.create_entry(entry(Some(&p.id), T0, None)).await.unwrap();
    assert_eq!(e.cost_center_id.as_deref(), Some(user_default.id.as_str()));

    core.save_cost_center(
        Some(&user_default.id),
        CostCenterInput {
            active: false,
            ..cc("200")
        },
    )
    .await
    .unwrap();
    let e = core.create_entry(entry(Some(&p.id), T0, None)).await.unwrap();
    assert_eq!(e.cost_center_id, None);
}

#[tokio::test]
async fn codes_are_unique_and_used_cost_centers_cannot_be_deleted() {
    let core = setup().await;
    let a = core.save_cost_center(None, cc("100")).await.unwrap();
    let dup = core.save_cost_center(None, cc(" 100 ")).await.unwrap_err();
    assert!(matches!(dup, Error::Validation { ref field, .. } if field == "code"));

    let unused = core.save_cost_center(None, cc("200")).await.unwrap();
    core.delete_cost_center(&unused.id).await.unwrap();
    assert_eq!(core.list_cost_centers().await.unwrap().len(), 1);

    core.create_entry(entry(None, T0, Some(&a.id))).await.unwrap();
    assert!(matches!(
        core.delete_cost_center(&a.id).await,
        Err(Error::Conflict(_))
    ));

    let bad = core
        .create_entry(entry(None, T0, Some("missing")))
        .await
        .unwrap_err();
    assert!(matches!(bad, Error::Validation { ref field, .. } if field == "costCenterId"));
}

#[tokio::test]
async fn parent_cycles_are_rejected() {
    let core = setup().await;
    let a = core.save_cost_center(None, cc("100")).await.unwrap();
    let b = core
        .save_cost_center(
            None,
            CostCenterInput {
                parent_id: Some(a.id.clone()),
                ..cc("110")
            },
        )
        .await
        .unwrap();
    let err = core
        .save_cost_center(
            Some(&a.id),
            CostCenterInput {
                parent_id: Some(b.id.clone()),
                ..cc("100")
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Validation { ref field, .. } if field == "parentId"));
}

#[tokio::test]
async fn json_export_and_backup_contain_the_data() {
    // On disk: SQLite skips VACUUM INTO for in-memory databases.
    let dir = std::env::temp_dir().join(format!("time-backup-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let core = common::with_activity(
        Core::open(&dir.join("zeima.db"), Arc::new(ManualClock::new(T0)))
            .await
            .unwrap(),
    )
    .await;
    core.set_default_tz("UTC").unwrap();
    let a = core.save_cost_center(None, cc("100")).await.unwrap();
    core.create_entry(entry(None, T0, Some(&a.id))).await.unwrap();

    let json = core.export_json().await.unwrap();
    let tables = &json["tables"];
    assert_eq!(tables["cost_center"][0]["code"], "100");
    assert_eq!(tables["time_entry"][0]["cost_center_id"], a.id.as_str());
    assert_eq!(tables["time_entry"][0]["start_at"], T0);
    assert_eq!(tables["activity"][0]["name"], "Work");
    assert_eq!(
        tables["time_entry"][0]["activity_id"],
        tables["activity"][0]["id"]
    );
    assert!(tables["work_settings"].is_array());

    let path = dir.join("backup.db");
    core.backup_to(&path).await.unwrap();
    let copy = Core::open(&path, Arc::new(ManualClock::new(T0))).await.unwrap();
    assert_eq!(copy.list_cost_centers().await.unwrap().len(), 1);
    assert_eq!(core.integrity_problem().await.unwrap(), None);
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn addon_records_and_storage() {
    use zeima_core::addon::Permission;
    let core = setup().await;
    let a = core
        .save_addon("com.example.a", "1.0.0", &[Permission::EntriesRead])
        .await
        .unwrap();
    assert!(!a.enabled);
    core.set_addon_enabled("com.example.a", true).await.unwrap();
    let mut settings = std::collections::BTreeMap::new();
    settings.insert("delimiter".to_string(), ";".to_string());
    core.set_addon_settings("com.example.a", &settings).await.unwrap();
    // Updating keeps enabled and settings.
    let a = core.save_addon("com.example.a", "1.1.0", &[]).await.unwrap();
    assert!(a.enabled);
    assert_eq!(a.version, "1.1.0");
    assert_eq!(a.settings["delimiter"], ";");
    assert!(a.granted.is_empty());

    core.addon_kv_set("com.example.a", "sync/token", Some("abc"))
        .await
        .unwrap();
    core.addon_kv_set("com.example.a", "other", Some("x"))
        .await
        .unwrap();
    assert_eq!(
        core.addon_kv_get("com.example.a", "sync/token")
            .await
            .unwrap()
            .as_deref(),
        Some("abc")
    );
    assert_eq!(
        core.addon_kv_keys("com.example.a", "sync/").await.unwrap(),
        vec!["sync/token"]
    );
    core.addon_kv_set("com.example.a", "other", None).await.unwrap();
    assert_eq!(core.addon_kv_get("com.example.a", "other").await.unwrap(), None);

    // Uninstalling removes the storage too.
    core.delete_addon("com.example.a").await.unwrap();
    assert!(core.list_addons().await.unwrap().is_empty());
    assert_eq!(
        core.addon_kv_get("com.example.a", "sync/token").await.unwrap(),
        None
    );
}
