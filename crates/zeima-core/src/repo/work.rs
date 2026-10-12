//! Work time: country profile selection, work settings, schedules, absences,
//! custom holidays, balance corrections, and computed days (cached in
//! `daily_summary`).

use std::collections::HashMap;

use chrono::{Datelike, Duration, NaiveDate};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

use super::{not_found, soft_delete, unique_violation};
use crate::db::{Core, WriteTx};
use crate::jurisdiction::{self, localize, Profile};
use crate::model::{
    Absence, AbsenceInput, AbsenceType, AbsenceTypeInput, BalanceAdjustment, BalanceAdjustmentInput,
    CustomHoliday, JurisdictionSelection, WorkSettings,
};
use crate::summary::Range;
use crate::worktime::{self, AbsenceOnDay, DayPlan, ScheduleModel, WorkDay, WorkSchedule};
use crate::{Error, Result};

/// Used until the user picks a country (onboarding asks for it).
const FALLBACK_PROFILE: &str = "AT";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct WorkScheduleInput {
    pub valid_from: NaiveDate,
    pub model: ScheduleModel,
    pub days: Vec<DayPlan>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct HolidayView {
    pub date: NaiveDate,
    pub name: String,
    pub day_off: bool,
    /// Id of a custom holiday (can be deleted); `None` for profile holidays.
    pub custom_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Balance {
    /// Balance up to and including yesterday.
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub until_yesterday_ms: i64,
    /// Today's contribution so far (grows while tracking).
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub today_ms: i64,
    /// First day counted (the earliest schedule); `None` without schedule.
    pub since: Option<NaiveDate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct VacationSummary {
    pub year: i32,
    pub entitlement_days: f64,
    /// Taken (dates up to today).
    pub used_days: f64,
    /// Booked for later this year.
    pub planned_days: f64,
    pub remaining_days: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    pub id: String,
    pub version: String,
    pub region: Option<String>,
    pub name: String,
    pub region_name: Option<String>,
    pub max_daily_minutes: Option<u32>,
    pub max_weekly_minutes: Option<u32>,
    pub min_daily_rest_minutes: Option<u32>,
    pub standard_weekly_minutes: u32,
    pub vacation_days: u32,
    /// `(after minutes, break minutes)`, ascending.
    pub breaks: Vec<(u32, u32)>,
}

const BUILTIN_ABSENCE_TYPES: &[(&str, &str, &str, bool, bool)] = &[
    // key, default name, color, reduces target, affects vacation
    ("vacation", "Vacation", "#22c55e", true, true),
    ("sick", "Sick leave", "#ef4444", true, false),
    ("comp_time", "Comp time", "#f59e0b", false, false),
    ("training", "Training", "#3b82f6", true, false),
    ("other", "Other", "#64748b", true, false),
];

fn absence_type_from_row(r: &SqliteRow) -> Result<AbsenceType> {
    Ok(AbsenceType {
        id: r.try_get("id")?,
        key: r.try_get("key")?,
        name: r.try_get("name")?,
        color: r.try_get("color")?,
        reduces_target: r.try_get("reduces_target")?,
        affects_vacation: r.try_get("affects_vacation")?,
    })
}

fn absence_from_row(r: &SqliteRow) -> Result<Absence> {
    Ok(Absence {
        id: r.try_get("id")?,
        user_id: r.try_get("user_id")?,
        type_id: r.try_get("type_id")?,
        start_date: r.try_get("start_date")?,
        end_date: r.try_get("end_date")?,
        half_day: r
            .try_get::<Option<String>, _>("half_day")?
            .map(|h| h.parse())
            .transpose()?,
        status: r.try_get::<String, _>("status")?.parse()?,
        note: r.try_get("note")?,
    })
}

fn schedule_from_row(r: &SqliteRow) -> Result<WorkSchedule> {
    let model: String = r.try_get("model")?;
    Ok(WorkSchedule {
        id: r.try_get("id")?,
        valid_from: r.try_get("valid_from")?,
        model: serde_json::from_value(serde_json::Value::String(model))?,
        days: serde_json::from_str(&r.try_get::<String, _>("days_json")?)?,
    })
}

fn model_str(model: ScheduleModel) -> &'static str {
    match model {
        ScheduleModel::Fixed => "fixed",
        ScheduleModel::Flextime => "flextime",
        ScheduleModel::Trust => "trust",
    }
}

/// Days whose cached summary depends on `[from, to]` (the day after `to`
/// too, because of the rest-time check). `to = None` invalidates all later days.
pub(crate) async fn invalidate_days(
    conn: &mut SqliteConnection,
    user_id: &str,
    from: NaiveDate,
    to: Option<NaiveDate>,
) -> Result<()> {
    let to = to.map(|d| d + Duration::days(1)).unwrap_or(NaiveDate::MAX);
    sqlx::query("DELETE FROM daily_summary WHERE user_id = ? AND date >= ? AND date <= ?")
        .bind(user_id)
        .bind(from)
        .bind(to)
        .execute(conn)
        .await?;
    Ok(())
}

/// Invalidates the days touched by an entry spanning `[start, end]`.
pub(crate) async fn invalidate_entry_days(tx: &mut WriteTx<'_>, start: i64, end: Option<i64>) -> Result<()> {
    let tz = parse_tz(&tx.default_tz())?;
    let user = tx.ctx().user_id.clone();
    let from = worktime::local_date(start, tz);
    let to = end.map(|e| worktime::local_date(e, tz));
    invalidate_days(tx.conn(), &user, from, to).await
}

async fn invalidate_all(tx: &mut WriteTx<'_>) -> Result<()> {
    sqlx::query("DELETE FROM daily_summary")
        .execute(tx.conn())
        .await?;
    Ok(())
}

fn parse_tz(tz: &str) -> Result<Tz> {
    tz.parse::<Tz>()
        .map_err(|_| Error::validation("tz", format!("unknown time zone '{tz}'")))
}

impl Core {
    pub fn tz(&self) -> Result<Tz> {
        parse_tz(&self.default_tz())
    }

    pub fn today(&self) -> Result<NaiveDate> {
        Ok(worktime::local_date(self.now_ms(), self.tz()?))
    }

    // ---------------------------------------------------------------------
    // Country profile
    // ---------------------------------------------------------------------

    pub async fn jurisdiction(&self) -> Result<Option<JurisdictionSelection>> {
        let row = sqlx::query(
            "SELECT profile_id, region FROM jurisdiction_selection
             WHERE workspace_id = ? AND deleted_at IS NULL
             ORDER BY user_id IS NULL LIMIT 1",
        )
        .bind(&self.ctx().workspace_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(match row {
            Some(r) => Some(JurisdictionSelection {
                profile_id: r.try_get("profile_id")?,
                region: r.try_get("region")?,
            }),
            None => None,
        })
    }

    /// The active profile; falls back to Austria until one is chosen.
    pub async fn profile(&self) -> Result<Profile> {
        match self.jurisdiction().await? {
            Some(sel) => jurisdiction::load(&sel.profile_id, sel.region.as_deref()),
            None => jurisdiction::load(FALLBACK_PROFILE, None),
        }
    }

    pub async fn profile_summary(&self, lang: &str) -> Result<ProfileSummary> {
        let p = self.profile().await?;
        let mut breaks: Vec<(u32, u32)> = p
            .breaks
            .iter()
            .map(|b| (b.after_minutes, b.break_minutes))
            .collect();
        breaks.sort();
        Ok(ProfileSummary {
            name: localize(&p.name, lang),
            region_name: p.region_name.as_ref().map(|n| localize(n, lang)),
            id: p.id,
            version: p.version,
            region: p.region,
            max_daily_minutes: p.work_time.max_daily_minutes,
            max_weekly_minutes: p.work_time.max_weekly_minutes,
            min_daily_rest_minutes: p.work_time.min_daily_rest_minutes,
            standard_weekly_minutes: p.work_time.standard_weekly_minutes,
            vacation_days: p.vacation.default_days,
            breaks,
        })
    }

    /// Selects the country profile for the workspace. Creates a default
    /// schedule from the profile if the user has none yet.
    pub async fn set_jurisdiction(&self, profile_id: &str, region: Option<&str>) -> Result<()> {
        let profile = jurisdiction::load(profile_id, region)?;
        let mut tx = self.begin().await?;
        let ws = tx.ctx().workspace_id.clone();
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id FROM jurisdiction_selection
             WHERE workspace_id = ? AND user_id IS NULL AND deleted_at IS NULL",
        )
        .bind(&ws)
        .fetch_optional(tx.conn())
        .await?;
        let s = tx.stamp();
        let id = existing.unwrap_or_else(crate::ids::new_id);
        sqlx::query(
            "INSERT INTO jurisdiction_selection (id, workspace_id, user_id, profile_id, region,
                created_at, updated_at, hlc, origin_device)
             VALUES (?, ?, NULL, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET profile_id = excluded.profile_id, region = excluded.region,
                updated_at = excluded.updated_at, hlc = excluded.hlc, origin_device = excluded.origin_device",
        )
        .bind(&id)
        .bind(&ws)
        .bind(&profile.id)
        .bind(&profile.region)
        .bind(s.now)
        .bind(s.now)
        .bind(&s.hlc)
        .bind(&s.device)
        .execute(tx.conn())
        .await?;
        let selection = JurisdictionSelection {
            profile_id: profile.id.clone(),
            region: profile.region.clone(),
        };
        tx.log_upsert("jurisdiction_selection", &id, &s, &selection)
            .await?;

        let user = tx.ctx().user_id.clone();
        let has_schedule: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM work_schedule WHERE user_id = ? AND deleted_at IS NULL)",
        )
        .bind(&user)
        .fetch_one(tx.conn())
        .await?;
        if !has_schedule {
            let today = worktime::local_date(tx.now(), parse_tz(&tx.default_tz())?);
            let input = default_schedule(&profile, today);
            write_schedule(&mut tx, None, input).await?;
        }
        invalidate_all(&mut tx).await?;
        tx.commit().await
    }

    // ---------------------------------------------------------------------
    // Work settings
    // ---------------------------------------------------------------------

    pub async fn work_settings(&self) -> Result<WorkSettings> {
        let json: Option<String> = sqlx::query_scalar(
            "SELECT settings_json FROM work_settings WHERE user_id = ? AND deleted_at IS NULL",
        )
        .bind(&self.ctx().user_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(json
            .map(|j| serde_json::from_str(&j))
            .transpose()?
            .unwrap_or_default())
    }

    pub async fn set_work_settings(&self, settings: WorkSettings) -> Result<WorkSettings> {
        settings.validate()?;
        let mut tx = self.begin().await?;
        let user = tx.ctx().user_id.clone();
        let existing: Option<String> =
            sqlx::query_scalar("SELECT id FROM work_settings WHERE user_id = ? AND deleted_at IS NULL")
                .bind(&user)
                .fetch_optional(tx.conn())
                .await?;
        let id = existing.unwrap_or_else(crate::ids::new_id);
        let s = tx.stamp();
        sqlx::query(
            "INSERT INTO work_settings (id, workspace_id, user_id, settings_json, created_at, updated_at,
                hlc, origin_device)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET settings_json = excluded.settings_json,
                updated_at = excluded.updated_at, hlc = excluded.hlc, origin_device = excluded.origin_device",
        )
        .bind(&id)
        .bind(tx.ctx().workspace_id.clone())
        .bind(&user)
        .bind(serde_json::to_string(&settings)?)
        .bind(s.now)
        .bind(s.now)
        .bind(&s.hlc)
        .bind(&s.device)
        .execute(tx.conn())
        .await?;
        tx.log_upsert("work_settings", &id, &s, &settings).await?;
        invalidate_all(&mut tx).await?;
        tx.commit().await?;
        Ok(settings)
    }

    // ---------------------------------------------------------------------
    // Schedules
    // ---------------------------------------------------------------------

    pub async fn list_schedules(&self) -> Result<Vec<WorkSchedule>> {
        let rows = sqlx::query(
            "SELECT id, valid_from, model, days_json FROM work_schedule
             WHERE user_id = ? AND deleted_at IS NULL ORDER BY valid_from",
        )
        .bind(&self.ctx().user_id)
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(schedule_from_row).collect()
    }

    /// Creates (`id = None`) or updates a schedule.
    pub async fn save_schedule(&self, id: Option<&str>, input: WorkScheduleInput) -> Result<WorkSchedule> {
        let mut tx = self.begin().await?;
        let schedule = write_schedule(&mut tx, id, input).await?;
        tx.commit().await?;
        Ok(schedule)
    }

    pub async fn delete_schedule(&self, id: &str) -> Result<()> {
        let mut tx = self.begin().await?;
        let user = tx.ctx().user_id.clone();
        let valid_from: Option<NaiveDate> = sqlx::query_scalar(
            "SELECT valid_from FROM work_schedule WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(&user)
        .fetch_optional(tx.conn())
        .await?;
        let valid_from = valid_from.ok_or_else(|| not_found("schedule", id))?;
        soft_delete(&mut tx, "work_schedule", id).await?;
        invalidate_days(tx.conn(), &user, valid_from, None).await?;
        tx.commit().await
    }

    // ---------------------------------------------------------------------
    // Absence types and absences
    // ---------------------------------------------------------------------

    /// Creates the built-in absence types once.
    pub(crate) async fn ensure_absence_types(&self) -> Result<()> {
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM absence_type WHERE workspace_id = ?")
            .bind(&self.ctx().workspace_id)
            .fetch_one(self.pool())
            .await?;
        if count > 0 {
            return Ok(());
        }
        let mut tx = self.begin().await?;
        for (key, name, color, reduces, vacation) in BUILTIN_ABSENCE_TYPES {
            let t = AbsenceType {
                id: crate::ids::new_id(),
                key: Some((*key).into()),
                name: (*name).into(),
                color: (*color).into(),
                reduces_target: *reduces,
                affects_vacation: *vacation,
            };
            write_absence_type(&mut tx, &t, true).await?;
        }
        tx.commit().await
    }

    pub async fn list_absence_types(&self) -> Result<Vec<AbsenceType>> {
        let rows = sqlx::query(
            "SELECT id, key, name, color, reduces_target, affects_vacation FROM absence_type
             WHERE workspace_id = ? AND deleted_at IS NULL ORDER BY key IS NULL, created_at",
        )
        .bind(&self.ctx().workspace_id)
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(absence_type_from_row).collect()
    }

    pub async fn save_absence_type(&self, id: Option<&str>, input: AbsenceTypeInput) -> Result<AbsenceType> {
        crate::model::validate_color(&input.color)?;
        if input.name.trim().is_empty() {
            return Err(Error::validation("name", "must not be empty"));
        }
        let mut tx = self.begin().await?;
        let (t, insert) = match id {
            Some(id) => {
                let existing = self
                    .list_absence_types()
                    .await?
                    .into_iter()
                    .find(|t| t.id == id)
                    .ok_or_else(|| not_found("absence type", id))?;
                (
                    AbsenceType {
                        name: input.name.trim().into(),
                        color: input.color,
                        reduces_target: input.reduces_target,
                        affects_vacation: input.affects_vacation,
                        ..existing
                    },
                    false,
                )
            }
            None => (
                AbsenceType {
                    id: crate::ids::new_id(),
                    key: None,
                    name: input.name.trim().into(),
                    color: input.color,
                    reduces_target: input.reduces_target,
                    affects_vacation: input.affects_vacation,
                },
                true,
            ),
        };
        write_absence_type(&mut tx, &t, insert).await?;
        invalidate_all(&mut tx).await?;
        tx.commit().await?;
        Ok(t)
    }

    pub async fn delete_absence_type(&self, id: &str) -> Result<()> {
        let mut tx = self.begin().await?;
        let row = sqlx::query("SELECT key FROM absence_type WHERE id = ? AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(tx.conn())
            .await?
            .ok_or_else(|| not_found("absence type", id))?;
        if row.try_get::<Option<String>, _>("key")?.is_some() {
            return Err(Error::Conflict(
                "Built-in absence types cannot be deleted.".into(),
            ));
        }
        let used: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM absence WHERE type_id = ? AND deleted_at IS NULL)",
        )
        .bind(id)
        .fetch_one(tx.conn())
        .await?;
        if used {
            return Err(Error::Conflict("This absence type is in use.".into()));
        }
        soft_delete(&mut tx, "absence_type", id).await?;
        tx.commit().await
    }

    /// Absences overlapping `[from, to]`.
    pub async fn list_absences(&self, from: NaiveDate, to: NaiveDate) -> Result<Vec<Absence>> {
        let rows = sqlx::query(
            "SELECT id, user_id, type_id, start_date, end_date, half_day, status, note FROM absence
             WHERE user_id = ? AND deleted_at IS NULL AND start_date <= ? AND end_date >= ?
             ORDER BY start_date",
        )
        .bind(&self.ctx().user_id)
        .bind(to)
        .bind(from)
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(absence_from_row).collect()
    }

    pub async fn save_absence(&self, id: Option<&str>, input: AbsenceInput) -> Result<Absence> {
        let input = input.normalized()?;
        let mut tx = self.begin().await?;
        let ws = tx.ctx().workspace_id.clone();
        let user = tx.ctx().user_id.clone();
        let type_ok: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM absence_type WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL)",
        )
        .bind(&input.type_id)
        .bind(&ws)
        .fetch_one(tx.conn())
        .await?;
        if !type_ok {
            return Err(Error::validation("typeId", "unknown absence type"));
        }
        let previous = match id {
            Some(id) => {
                let row = sqlx::query(
                    "SELECT id, user_id, type_id, start_date, end_date, half_day, status, note FROM absence
                     WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
                )
                .bind(id)
                .bind(&user)
                .fetch_optional(tx.conn())
                .await?
                .ok_or_else(|| not_found("absence", id))?;
                Some(absence_from_row(&row)?)
            }
            None => None,
        };
        let absence = Absence {
            id: previous
                .as_ref()
                .map_or_else(crate::ids::new_id, |p| p.id.clone()),
            user_id: user.clone(),
            type_id: input.type_id,
            start_date: input.start_date,
            end_date: input.end_date,
            half_day: input.half_day,
            status: crate::model::AbsenceStatus::Approved,
            note: input.note,
        };
        let s = tx.stamp();
        sqlx::query(
            "INSERT INTO absence (id, workspace_id, user_id, type_id, start_date, end_date, half_day, status,
                note, created_at, updated_at, hlc, origin_device)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET type_id = excluded.type_id, start_date = excluded.start_date,
                end_date = excluded.end_date, half_day = excluded.half_day, note = excluded.note,
                updated_at = excluded.updated_at, hlc = excluded.hlc, origin_device = excluded.origin_device",
        )
        .bind(&absence.id)
        .bind(&ws)
        .bind(&user)
        .bind(&absence.type_id)
        .bind(absence.start_date)
        .bind(absence.end_date)
        .bind(absence.half_day.map(|h| h.as_str()))
        .bind(absence.status.as_str())
        .bind(&absence.note)
        .bind(s.now)
        .bind(s.now)
        .bind(&s.hlc)
        .bind(&s.device)
        .execute(tx.conn())
        .await?;
        tx.log_upsert("absence", &absence.id, &s, &absence).await?;
        let from = previous
            .as_ref()
            .map_or(absence.start_date, |p| p.start_date.min(absence.start_date));
        let to = previous
            .as_ref()
            .map_or(absence.end_date, |p| p.end_date.max(absence.end_date));
        invalidate_days(tx.conn(), &user, from, Some(to)).await?;
        tx.commit().await?;
        Ok(absence)
    }

    pub async fn delete_absence(&self, id: &str) -> Result<()> {
        let mut tx = self.begin().await?;
        let user = tx.ctx().user_id.clone();
        let row = sqlx::query(
            "SELECT start_date, end_date FROM absence WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(&user)
        .fetch_optional(tx.conn())
        .await?
        .ok_or_else(|| not_found("absence", id))?;
        let (from, to): (NaiveDate, NaiveDate) = (row.try_get("start_date")?, row.try_get("end_date")?);
        soft_delete(&mut tx, "absence", id).await?;
        invalidate_days(tx.conn(), &user, from, Some(to)).await?;
        tx.commit().await
    }

    // ---------------------------------------------------------------------
    // Holidays and balance corrections
    // ---------------------------------------------------------------------

    pub async fn list_custom_holidays(&self) -> Result<Vec<CustomHoliday>> {
        let rows = sqlx::query(
            "SELECT id, date, name FROM custom_holiday WHERE workspace_id = ? AND deleted_at IS NULL ORDER BY date",
        )
        .bind(&self.ctx().workspace_id)
        .fetch_all(self.pool())
        .await?;
        rows.iter()
            .map(|r| {
                Ok(CustomHoliday {
                    id: r.try_get("id")?,
                    date: r.try_get("date")?,
                    name: r.try_get("name")?,
                })
            })
            .collect()
    }

    pub async fn create_custom_holiday(&self, date: NaiveDate, name: &str) -> Result<CustomHoliday> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::validation("name", "must not be empty"));
        }
        let mut tx = self.begin().await?;
        let s = tx.stamp();
        let h = CustomHoliday {
            id: crate::ids::new_id(),
            date,
            name: name.into(),
        };
        sqlx::query(
            "INSERT INTO custom_holiday (id, workspace_id, date, name, created_at, updated_at, hlc, origin_device)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&h.id)
        .bind(tx.ctx().workspace_id.clone())
        .bind(h.date)
        .bind(&h.name)
        .bind(s.now)
        .bind(s.now)
        .bind(&s.hlc)
        .bind(&s.device)
        .execute(tx.conn())
        .await?;
        tx.log_upsert("custom_holiday", &h.id, &s, &h).await?;
        let user = tx.ctx().user_id.clone();
        invalidate_days(tx.conn(), &user, date, Some(date)).await?;
        tx.commit().await?;
        Ok(h)
    }

    pub async fn delete_custom_holiday(&self, id: &str) -> Result<()> {
        let mut tx = self.begin().await?;
        let date: Option<NaiveDate> =
            sqlx::query_scalar("SELECT date FROM custom_holiday WHERE id = ? AND deleted_at IS NULL")
                .bind(id)
                .fetch_optional(tx.conn())
                .await?;
        let date = date.ok_or_else(|| not_found("holiday", id))?;
        soft_delete(&mut tx, "custom_holiday", id).await?;
        let user = tx.ctx().user_id.clone();
        invalidate_days(tx.conn(), &user, date, Some(date)).await?;
        tx.commit().await
    }

    /// Profile and custom holidays of a year, names in `lang`.
    pub async fn holidays(&self, year: i32, lang: &str) -> Result<Vec<HolidayView>> {
        let mut list: Vec<HolidayView> = self
            .profile()
            .await?
            .holidays(year)
            .into_iter()
            .map(|h| HolidayView {
                date: h.date,
                name: localize(&h.name, lang),
                day_off: h.day_off,
                custom_id: None,
            })
            .collect();
        list.extend(
            self.list_custom_holidays()
                .await?
                .into_iter()
                .filter(|h| h.date.year() == year)
                .map(|h| HolidayView {
                    date: h.date,
                    name: h.name,
                    day_off: true,
                    custom_id: Some(h.id),
                }),
        );
        list.sort_by_key(|h| h.date);
        Ok(list)
    }

    pub async fn list_adjustments(&self) -> Result<Vec<BalanceAdjustment>> {
        let rows = sqlx::query(
            "SELECT id, date, minutes, reason FROM balance_adjustment
             WHERE user_id = ? AND deleted_at IS NULL ORDER BY date DESC",
        )
        .bind(&self.ctx().user_id)
        .fetch_all(self.pool())
        .await?;
        rows.iter()
            .map(|r| {
                Ok(BalanceAdjustment {
                    id: r.try_get("id")?,
                    date: r.try_get("date")?,
                    minutes: r.try_get("minutes")?,
                    reason: r.try_get("reason")?,
                })
            })
            .collect()
    }

    pub async fn create_adjustment(&self, input: BalanceAdjustmentInput) -> Result<BalanceAdjustment> {
        let reason = input.reason.trim();
        if reason.is_empty() {
            return Err(Error::validation("reason", "please give a reason"));
        }
        if input.minutes == 0 {
            return Err(Error::validation("minutes", "must not be zero"));
        }
        let mut tx = self.begin().await?;
        let s = tx.stamp();
        let a = BalanceAdjustment {
            id: crate::ids::new_id(),
            date: input.date,
            minutes: input.minutes,
            reason: reason.into(),
        };
        let user = tx.ctx().user_id.clone();
        sqlx::query(
            "INSERT INTO balance_adjustment (id, workspace_id, user_id, date, minutes, reason, created_at,
                updated_at, hlc, origin_device)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&a.id)
        .bind(tx.ctx().workspace_id.clone())
        .bind(&user)
        .bind(a.date)
        .bind(a.minutes)
        .bind(&a.reason)
        .bind(s.now)
        .bind(s.now)
        .bind(&s.hlc)
        .bind(&s.device)
        .execute(tx.conn())
        .await?;
        tx.log_upsert("balance_adjustment", &a.id, &s, &a).await?;
        invalidate_days(tx.conn(), &user, a.date, Some(a.date)).await?;
        tx.commit().await?;
        Ok(a)
    }

    pub async fn delete_adjustment(&self, id: &str) -> Result<()> {
        let mut tx = self.begin().await?;
        let user = tx.ctx().user_id.clone();
        let date: Option<NaiveDate> = sqlx::query_scalar(
            "SELECT date FROM balance_adjustment WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(&user)
        .fetch_optional(tx.conn())
        .await?;
        let date = date.ok_or_else(|| not_found("adjustment", id))?;
        soft_delete(&mut tx, "balance_adjustment", id).await?;
        invalidate_days(tx.conn(), &user, date, Some(date)).await?;
        tx.commit().await
    }

    // ---------------------------------------------------------------------
    // Computed days
    // ---------------------------------------------------------------------

    /// Days `from..=to`. Past days come from the cache when possible.
    pub async fn work_days(&self, from: NaiveDate, to: NaiveDate) -> Result<Vec<WorkDay>> {
        if to < from {
            return Ok(vec![]);
        }
        let today = self.today()?;
        let version = self.profile().await?.version;
        let cache_to = to.min(today - Duration::days(1));
        if from <= cache_to {
            let rows = sqlx::query(
                "SELECT data FROM daily_summary
                 WHERE user_id = ? AND date >= ? AND date <= ? AND profile_version = ? ORDER BY date",
            )
            .bind(&self.ctx().user_id)
            .bind(from)
            .bind(cache_to)
            .bind(&version)
            .fetch_all(self.pool())
            .await?;
            let expected = (cache_to - from).num_days() + 1;
            if rows.len() as i64 == expected {
                let mut days: Vec<WorkDay> = rows
                    .iter()
                    .map(|r| Ok(serde_json::from_str(&r.try_get::<String, _>("data")?)?))
                    .collect::<Result<_>>()?;
                if to > cache_to {
                    days.extend(self.compute_range(cache_to + Duration::days(1), to).await?);
                }
                return Ok(days);
            }
        }
        let days = self.compute_range(from, to).await?;
        self.store_cache(&days, today, &version).await?;
        Ok(days)
    }

    async fn store_cache(&self, days: &[WorkDay], today: NaiveDate, version: &str) -> Result<()> {
        let now = self.now_ms();
        let mut tx = self.pool().begin().await?;
        for day in days.iter().filter(|d| d.date < today) {
            sqlx::query(
                "INSERT INTO daily_summary (user_id, date, data, profile_version, computed_at)
                 VALUES (?, ?, ?, ?, ?)
                 ON CONFLICT (user_id, date) DO UPDATE SET data = excluded.data,
                    profile_version = excluded.profile_version, computed_at = excluded.computed_at",
            )
            .bind(&self.ctx().user_id)
            .bind(day.date)
            .bind(serde_json::to_string(day)?)
            .bind(version)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Computes days without the cache.
    pub async fn compute_range(&self, from: NaiveDate, to: NaiveDate) -> Result<Vec<WorkDay>> {
        let tz = self.tz()?;
        let profile = self.profile().await?;
        let settings = self.work_settings().await?;
        let schedules = self.list_schedules().await?;
        // One extra day before `from` provides the rest-time reference.
        let first = from - Duration::days(1);
        let (start_ms, _) = worktime::day_bounds(first, tz);
        let (_, end_ms) = worktime::day_bounds(to, tz);
        let now = self.now_ms();
        let intervals: Vec<(i64, i64)> = self
            .list_entries(Range {
                from: start_ms,
                to: end_ms,
            })
            .await?
            .into_iter()
            .map(|e| (e.start_at, e.end_at.unwrap_or(now).max(e.start_at)))
            .collect();

        let mut holidays = HashMap::new();
        for year in first.year()..=to.year() {
            for h in profile.holidays(year).into_iter().filter(|h| h.day_off) {
                holidays.insert(h.date, h.id);
            }
        }
        for h in self.list_custom_holidays().await? {
            holidays.insert(h.date, h.name);
        }

        let types: HashMap<String, AbsenceType> = self
            .list_absence_types()
            .await?
            .into_iter()
            .map(|t| (t.id.clone(), t))
            .collect();
        let mut absences: HashMap<NaiveDate, Vec<AbsenceOnDay>> = HashMap::new();
        for a in self.list_absences(first, to).await? {
            let Some(t) = types.get(&a.type_id) else { continue };
            let fraction = if a.half_day.is_some() { 0.5 } else { 1.0 };
            let mut d = a.start_date.max(first);
            while d <= a.end_date.min(to) {
                absences.entry(d).or_default().push(AbsenceOnDay {
                    type_id: a.type_id.clone(),
                    fraction,
                    reduces_target: t.reduces_target,
                });
                d += Duration::days(1);
            }
        }

        let mut adjustments: HashMap<NaiveDate, i64> = HashMap::new();
        for a in self.list_adjustments().await? {
            if a.date >= first && a.date <= to {
                *adjustments.entry(a.date).or_default() += a.minutes;
            }
        }

        let inputs = worktime::Inputs {
            tz,
            profile: &profile,
            schedules: &schedules,
            intervals: &intervals,
            holidays: &holidays,
            absences: &absences,
            adjustments: &adjustments,
            deduct_breaks: settings.deduct_breaks,
        };
        let mut days = worktime::compute_days(first, to, &inputs, None);
        days.remove(0);
        Ok(days)
    }

    /// Flexitime balance from the first schedule until today.
    pub async fn balance(&self) -> Result<Balance> {
        let today = self.today()?;
        let since = self.list_schedules().await?.first().map(|s| s.valid_from);
        let Some(since) = since.filter(|s| *s <= today) else {
            return Ok(Balance {
                until_yesterday_ms: 0,
                today_ms: 0,
                since,
            });
        };
        let days = self.work_days(since, today).await?;
        let (past, current) = days.split_at(days.len() - 1);
        Ok(Balance {
            until_yesterday_ms: worktime::balance_ms(past),
            today_ms: worktime::balance_ms(current),
            since: Some(since),
        })
    }

    pub async fn vacation_summary(&self, year: i32) -> Result<VacationSummary> {
        let profile = self.profile().await?;
        let settings = self.work_settings().await?;
        let entitlement = settings
            .vacation_days_per_year
            .unwrap_or(profile.vacation.default_days as f64);
        let schedules = self.list_schedules().await?;
        let types: HashMap<String, AbsenceType> = self
            .list_absence_types()
            .await?
            .into_iter()
            .map(|t| (t.id.clone(), t))
            .collect();
        let start = NaiveDate::from_ymd_opt(year, 1, 1).expect("valid date");
        let end = NaiveDate::from_ymd_opt(year, 12, 31).expect("valid date");
        let holidays: Vec<NaiveDate> = self
            .holidays(year, "en")
            .await?
            .into_iter()
            .filter(|h| h.day_off)
            .map(|h| h.date)
            .collect();
        let today = self.today()?;
        let (mut used, mut planned) = (0.0, 0.0);
        for a in self.list_absences(start, end).await? {
            if !types.get(&a.type_id).is_some_and(|t| t.affects_vacation) {
                continue;
            }
            let fraction = if a.half_day.is_some() { 0.5 } else { 1.0 };
            let mut d = a.start_date.max(start);
            while d <= a.end_date.min(end) {
                let workday = worktime::schedule_on(&schedules, d)
                    .and_then(|s| s.plan(d))
                    .is_some_and(|p| p.target_minutes > 0);
                if workday && !holidays.contains(&d) {
                    if d <= today {
                        used += fraction;
                    } else {
                        planned += fraction;
                    }
                }
                d += Duration::days(1);
            }
        }
        Ok(VacationSummary {
            year,
            entitlement_days: entitlement,
            used_days: used,
            planned_days: planned,
            remaining_days: entitlement - used - planned,
        })
    }
}

/// A schedule matching the profile's standard week, starting `from`.
pub fn default_schedule(profile: &Profile, from: NaiveDate) -> WorkScheduleInput {
    let workdays = &profile.work_time.standard_workdays;
    let per_day = profile.work_time.standard_weekly_minutes / (workdays.len().max(1) as u32);
    let days = [
        jurisdiction::Weekday::Mon,
        jurisdiction::Weekday::Tue,
        jurisdiction::Weekday::Wed,
        jurisdiction::Weekday::Thu,
        jurisdiction::Weekday::Fri,
        jurisdiction::Weekday::Sat,
        jurisdiction::Weekday::Sun,
    ]
    .iter()
    .map(|d| DayPlan {
        target_minutes: if workdays.contains(d) { per_day } else { 0 },
        core_start: None,
        core_end: None,
    })
    .collect();
    WorkScheduleInput {
        valid_from: from,
        model: ScheduleModel::Flextime,
        days,
    }
}

async fn write_schedule(
    tx: &mut WriteTx<'_>,
    id: Option<&str>,
    input: WorkScheduleInput,
) -> Result<WorkSchedule> {
    let user = tx.ctx().user_id.clone();
    let previous_from: Option<NaiveDate> = match id {
        Some(id) => Some(
            sqlx::query_scalar(
                "SELECT valid_from FROM work_schedule WHERE id = ? AND user_id = ? AND deleted_at IS NULL",
            )
            .bind(id)
            .bind(&user)
            .fetch_optional(tx.conn())
            .await?
            .ok_or_else(|| not_found("schedule", id))?,
        ),
        None => None,
    };
    let schedule = WorkSchedule {
        id: id.map_or_else(crate::ids::new_id, str::to_string),
        valid_from: input.valid_from,
        model: input.model,
        days: input.days,
    };
    schedule.validate()?;
    let s = tx.stamp();
    sqlx::query(
        "INSERT INTO work_schedule (id, workspace_id, user_id, valid_from, model, days_json, created_at,
            updated_at, hlc, origin_device)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (id) DO UPDATE SET valid_from = excluded.valid_from, model = excluded.model,
            days_json = excluded.days_json, updated_at = excluded.updated_at, hlc = excluded.hlc,
            origin_device = excluded.origin_device",
    )
    .bind(&schedule.id)
    .bind(tx.ctx().workspace_id.clone())
    .bind(&user)
    .bind(schedule.valid_from)
    .bind(model_str(schedule.model))
    .bind(serde_json::to_string(&schedule.days)?)
    .bind(s.now)
    .bind(s.now)
    .bind(&s.hlc)
    .bind(&s.device)
    .execute(tx.conn())
    .await
    .map_err(|e| unique_violation(e, "validFrom", "another schedule starts on this date"))?;
    tx.log_upsert("work_schedule", &schedule.id, &s, &schedule)
        .await?;
    let from = previous_from.map_or(schedule.valid_from, |p| p.min(schedule.valid_from));
    invalidate_days(tx.conn(), &user, from, None).await?;
    Ok(schedule)
}

async fn write_absence_type(tx: &mut WriteTx<'_>, t: &AbsenceType, insert: bool) -> Result<()> {
    let s = tx.stamp();
    let sql = if insert {
        "INSERT INTO absence_type (name, color, reduces_target, affects_vacation, updated_at, hlc,
            origin_device, id, workspace_id, key, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    } else {
        "UPDATE absence_type SET name = ?, color = ?, reduces_target = ?, affects_vacation = ?,
            updated_at = ?, hlc = ?, origin_device = ?
         WHERE id = ? AND workspace_id = ? AND key IS ? AND ? IS NOT NULL"
    };
    sqlx::query(sql)
        .bind(&t.name)
        .bind(&t.color)
        .bind(t.reduces_target)
        .bind(t.affects_vacation)
        .bind(s.now)
        .bind(&s.hlc)
        .bind(&s.device)
        .bind(&t.id)
        .bind(tx.ctx().workspace_id.clone())
        .bind(&t.key)
        .bind(s.now)
        .execute(tx.conn())
        .await?;
    tx.log_upsert("absence_type", &t.id, &s, t).await
}
