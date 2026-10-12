//! Project budget usage in the current budget period.

use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::db::Core;
use crate::locale::WeekStart;
use crate::model::{BudgetPeriod, Project, ProjectStatus};
use crate::worktime::day_bounds;
use crate::Result;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct BudgetStatus {
    pub project_id: String,
    pub period: BudgetPeriod,
    /// First day of the current period; `None` for a total budget.
    pub period_start: Option<NaiveDate>,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub used_ms: i64,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub budget_ms: i64,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub warn_pct: i64,
}

impl BudgetStatus {
    /// Highest threshold reached: 0 = none, `warn_pct`, or 100.
    pub fn level(&self) -> i64 {
        if self.budget_ms == 0 {
            return 0;
        }
        let pct = self.used_ms * 100 / self.budget_ms;
        if pct >= 100 {
            100
        } else if pct >= self.warn_pct {
            self.warn_pct
        } else {
            0
        }
    }
}

/// First day of the period containing `today`, and the day after its end.
fn period_dates(
    period: BudgetPeriod,
    today: NaiveDate,
    week_start: WeekStart,
) -> Option<(NaiveDate, NaiveDate)> {
    match period {
        BudgetPeriod::Total => None,
        BudgetPeriod::Weekly => {
            let start = week_start.week_of(today);
            Some((start, start + Duration::days(7)))
        }
        BudgetPeriod::Monthly => {
            let start = today.with_day(1)?;
            let next = if start.month() == 12 {
                NaiveDate::from_ymd_opt(start.year() + 1, 1, 1)?
            } else {
                NaiveDate::from_ymd_opt(start.year(), start.month() + 1, 1)?
            };
            Some((start, next))
        }
    }
}

impl Core {
    /// Budget usage of every non-archived project that has a budget.
    pub async fn budget_statuses(&self) -> Result<Vec<BudgetStatus>> {
        let projects: Vec<Project> = self
            .list_projects(false)
            .await?
            .into_iter()
            .filter(|p| p.budget_minutes.is_some() && p.status != ProjectStatus::Archived)
            .collect();
        let tz = self.tz()?;
        let today = self.today()?;
        let now = self.now_ms();
        let mut out = Vec::with_capacity(projects.len());
        for p in projects {
            let dates = period_dates(p.budget_period, today, self.week_start());
            let (from, to) = match dates {
                Some((start, next)) => (day_bounds(start, tz).0, day_bounds(next, tz).0),
                None => (i64::MIN, i64::MAX),
            };
            let used_ms: i64 = sqlx::query_scalar(
                "SELECT coalesce(sum(min(coalesce(end_at, ?1), ?3) - max(start_at, ?2)), 0)
                 FROM time_entry
                 WHERE project_id = ?4 AND user_id = ?5 AND deleted_at IS NULL
                   AND start_at < ?3 AND coalesce(end_at, ?1) > ?2",
            )
            .bind(now)
            .bind(from)
            .bind(to)
            .bind(&p.id)
            .bind(&self.ctx().user_id)
            .fetch_one(self.pool())
            .await?;
            out.push(BudgetStatus {
                project_id: p.id,
                period: p.budget_period,
                period_start: dates.map(|d| d.0),
                used_ms,
                budget_ms: p.budget_minutes.unwrap_or(0) * 60_000,
                warn_pct: p.budget_warn_pct,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn periods_start_on_monday_and_the_first() {
        let d: NaiveDate = "2026-12-17".parse().unwrap(); // Thursday
        assert_eq!(
            period_dates(BudgetPeriod::Weekly, d, WeekStart::Monday),
            Some(("2026-12-14".parse().unwrap(), "2026-12-21".parse().unwrap()))
        );
        assert_eq!(
            period_dates(BudgetPeriod::Weekly, d, WeekStart::Sunday),
            Some(("2026-12-13".parse().unwrap(), "2026-12-20".parse().unwrap()))
        );
        assert_eq!(
            period_dates(BudgetPeriod::Monthly, d, WeekStart::Monday),
            Some(("2026-12-01".parse().unwrap(), "2027-01-01".parse().unwrap()))
        );
        assert_eq!(period_dates(BudgetPeriod::Total, d, WeekStart::Monday), None);
    }

    #[test]
    fn level_reports_the_highest_threshold() {
        let s = |used| BudgetStatus {
            project_id: "p".into(),
            period: BudgetPeriod::Total,
            period_start: None,
            used_ms: used,
            budget_ms: 100,
            warn_pct: 80,
        };
        assert_eq!(s(79).level(), 0);
        assert_eq!(s(80).level(), 80);
        assert_eq!(s(150).level(), 100);
    }
}
