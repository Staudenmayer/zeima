//! Where people work on a day (SPEC §5.11), in shared workspaces.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::db::Core;
use crate::{Error, Result};

crate::model::string_enum!(LocationKind { Office => "office", Home => "home", Travel => "travel", Other => "other" });

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct WorkLocation {
    pub id: String,
    /// The person (their account id).
    pub user_id: String,
    pub date: NaiveDate,
    pub kind: LocationKind,
    /// Which office, for a day in the office.
    #[serde(default)]
    pub office_id: Option<String>,
}

/// A place people work from, with room for `capacity` people a day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Office {
    pub id: String,
    pub name: String,
    #[cfg_attr(feature = "specta", specta(type = Option<specta_typescript::Number>))]
    pub capacity: Option<i64>,
}

/// The same row for the same person and day on every device.
fn location_id(workspace_id: &str, user_id: &str, date: NaiveDate) -> String {
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!("work_location|{workspace_id}|{user_id}|{date}").as_bytes(),
    )
    .to_string()
}

impl Core {
    /// Locations in `from..=to` that this device has: the person's own and
    /// those of colleagues the server sent.
    pub async fn list_locations(&self, from: NaiveDate, to: NaiveDate) -> Result<Vec<WorkLocation>> {
        let rows = sqlx::query(
            "SELECT id, user_id, date, kind, office_id FROM work_location
             WHERE workspace_id = ? AND deleted_at IS NULL AND date >= ? AND date <= ? ORDER BY date, user_id",
        )
        .bind(&self.ctx().workspace_id)
        .bind(from)
        .bind(to)
        .fetch_all(self.pool())
        .await?;
        rows.iter()
            .map(|r| {
                Ok(WorkLocation {
                    id: r.try_get("id")?,
                    user_id: r.try_get("user_id")?,
                    date: r.try_get("date")?,
                    kind: r.try_get::<String, _>("kind")?.parse()?,
                    office_id: r.try_get("office_id")?,
                })
            })
            .collect()
    }

    /// Sets (or, with `None`, clears) where the person works on `date`.
    pub async fn set_location(
        &self,
        date: NaiveDate,
        kind: Option<LocationKind>,
        office_id: Option<String>,
    ) -> Result<()> {
        if !self.ctx().shared {
            return Err(Error::validation(
                "workspace",
                "work locations are shared in an organization's workspace",
            ));
        }
        let mut tx = self.begin().await?;
        let ws = tx.ctx().workspace_id.clone();
        let user = tx.ctx().user_id.clone();
        let id = location_id(&ws, &user, date);
        // Only a day in the office has an office, and it must be one of ours.
        let office_id = office_id.filter(|_| kind == Some(LocationKind::Office));
        if let Some(office) = &office_id {
            let known: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM office WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL)")
                .bind(office)
                .bind(&ws)
                .fetch_one(tx.conn())
                .await?;
            if !known {
                return Err(Error::validation("officeId", "unknown office"));
            }
        }
        let s = tx.stamp();
        match kind {
            Some(kind) => {
                sqlx::query(
                    "INSERT INTO work_location (id, workspace_id, user_id, date, kind, office_id, created_at, updated_at, hlc, origin_device)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                     ON CONFLICT (id) DO UPDATE SET kind = excluded.kind, office_id = excluded.office_id, deleted_at = NULL,
                        updated_at = excluded.updated_at, hlc = excluded.hlc, origin_device = excluded.origin_device",
                )
                .bind(&id)
                .bind(&ws)
                .bind(&user)
                .bind(date)
                .bind(kind.as_str())
                .bind(&office_id)
                .bind(s.now)
                .bind(s.now)
                .bind(&s.hlc)
                .bind(&s.device)
                .execute(tx.conn())
                .await?;
                let row = WorkLocation {
                    id: id.clone(),
                    user_id: user,
                    date,
                    kind,
                    office_id,
                };
                tx.log_upsert("work_location", &id, &s, &row).await?;
            }
            None => {
                let exists: bool = sqlx::query_scalar(
                    "SELECT EXISTS (SELECT 1 FROM work_location WHERE id = ? AND deleted_at IS NULL)",
                )
                .bind(&id)
                .fetch_one(tx.conn())
                .await?;
                if exists {
                    super::soft_delete(&mut tx, "work_location", &id).await?;
                }
            }
        }
        tx.commit().await
    }
}

impl Core {
    pub async fn list_offices(&self) -> Result<Vec<Office>> {
        let rows = sqlx::query("SELECT id, name, capacity FROM office WHERE workspace_id = ? AND deleted_at IS NULL ORDER BY lower(name)")
            .bind(&self.ctx().workspace_id)
            .fetch_all(self.pool())
            .await?;
        rows.iter()
            .map(|r| {
                Ok(Office {
                    id: r.try_get("id")?,
                    name: r.try_get("name")?,
                    capacity: r.try_get("capacity")?,
                })
            })
            .collect()
    }

    pub async fn save_office(&self, id: Option<&str>, name: &str, capacity: Option<i64>) -> Result<Office> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 100 {
            return Err(Error::validation("name", "must be 1 to 100 characters"));
        }
        if capacity.is_some_and(|c| c < 1) {
            return Err(Error::validation("capacity", "must be at least 1"));
        }
        let mut tx = self.begin().await?;
        let ws = tx.ctx().workspace_id.clone();
        let id = match id {
            Some(id) => {
                let known: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM office WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL)")
                    .bind(id)
                    .bind(&ws)
                    .fetch_one(tx.conn())
                    .await?;
                if !known {
                    return Err(super::not_found("office", id));
                }
                id.to_string()
            }
            None => crate::ids::new_id(),
        };
        let s = tx.stamp();
        sqlx::query(
            "INSERT INTO office (id, workspace_id, name, capacity, created_at, updated_at, hlc, origin_device)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET name = excluded.name, capacity = excluded.capacity,
                updated_at = excluded.updated_at, hlc = excluded.hlc, origin_device = excluded.origin_device",
        )
        .bind(&id)
        .bind(&ws)
        .bind(name)
        .bind(capacity)
        .bind(s.now)
        .bind(s.now)
        .bind(&s.hlc)
        .bind(&s.device)
        .execute(tx.conn())
        .await?;
        let office = Office {
            id: id.clone(),
            name: name.to_string(),
            capacity,
        };
        tx.log_upsert("office", &id, &s, &office).await?;
        tx.commit().await?;
        Ok(office)
    }

    pub async fn delete_office(&self, id: &str) -> Result<()> {
        let mut tx = self.begin().await?;
        super::soft_delete(&mut tx, "office", id).await?;
        tx.commit().await
    }
}

/// A usual week: for each weekday (Monday = 1 … Sunday = 7) where the person works.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct LocationPattern {
    pub days: Vec<PatternDay>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct PatternDay {
    /// 1 = Monday … 7 = Sunday.
    pub weekday: u8,
    pub kind: LocationKind,
    #[serde(default)]
    pub office_id: Option<String>,
}

impl Core {
    fn pattern_key(&self) -> String {
        format!("location_pattern:{}", self.ctx().workspace_id)
    }

    pub async fn location_pattern(&self) -> Result<LocationPattern> {
        Ok(self
            .get_meta(&self.pattern_key())
            .await?
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default())
    }

    pub async fn set_location_pattern(&self, pattern: LocationPattern) -> Result<()> {
        if pattern.days.iter().any(|d| !(1..=7).contains(&d.weekday)) {
            return Err(Error::validation("weekday", "must be 1 (Monday) to 7 (Sunday)"));
        }
        self.set_meta(&self.pattern_key(), &serde_json::to_string(&pattern)?)
            .await
    }

    /// Fills the days of `from..=to` that have no location yet from the usual
    /// week. Returns how many days were set.
    pub async fn apply_location_pattern(&self, from: NaiveDate, to: NaiveDate) -> Result<u32> {
        use chrono::Datelike;
        let pattern = self.location_pattern().await?;
        if pattern.days.is_empty() {
            return Ok(0);
        }
        let have: std::collections::HashSet<(String, NaiveDate)> = self
            .list_locations(from, to)
            .await?
            .into_iter()
            .map(|l| (l.user_id, l.date))
            .collect();
        let mut set = 0;
        let mut day = from;
        while day <= to {
            let weekday = day.weekday().number_from_monday() as u8;
            if let Some(p) = pattern.days.iter().find(|d| d.weekday == weekday) {
                if !have.contains(&(self.ctx().user_id.clone(), day)) {
                    // An office that has since been deleted is just "office".
                    let office = match &p.office_id {
                        Some(o) if self.list_offices().await?.iter().any(|x| &x.id == o) => Some(o.clone()),
                        _ => None,
                    };
                    self.set_location(day, Some(p.kind), office).await?;
                    set += 1;
                }
            }
            day += chrono::Duration::days(1);
        }
        Ok(set)
    }
}
