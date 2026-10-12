use chrono::{Datelike, Duration, NaiveDate};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{easter, LocalizedText, Weekday};

/// How a holiday's date is computed for a year. Written in TOML as e.g.
/// `{ fixed = "01-06" }` or `{ easter_offset = 39 }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HolidayRule {
    /// Same date every year, `"MM-DD"`.
    Fixed(String),
    /// Days after (or before, if negative) Western Easter Sunday.
    EasterOffset(i64),
    /// Days after Orthodox Easter Sunday.
    OrthodoxEasterOffset(i64),
    /// The n-th weekday of a month; negative `n` counts from the end
    /// (`-1` = last).
    NthWeekday { month: u32, weekday: Weekday, n: i32 },
    /// The last `weekday` strictly before `month`-`day`
    /// (e.g. Buß- und Bettag: Wednesday before 23 November).
    WeekdayBefore { month: u32, day: u32, weekday: Weekday },
    /// The first `weekday` strictly after `month`-`day`.
    WeekdayAfter { month: u32, day: u32, weekday: Weekday },
}

impl HolidayRule {
    pub fn date(&self, year: i32) -> Option<NaiveDate> {
        match self {
            HolidayRule::Fixed(md) => {
                let (m, d) = md.split_once('-')?;
                NaiveDate::from_ymd_opt(year, m.parse().ok()?, d.parse().ok()?)
            }
            HolidayRule::EasterOffset(days) => Some(easter::western(year) + Duration::days(*days)),
            HolidayRule::OrthodoxEasterOffset(days) => Some(easter::orthodox(year) + Duration::days(*days)),
            HolidayRule::NthWeekday { month, weekday, n } => {
                nth_weekday(year, *month, weekday.to_chrono(), *n)
            }
            HolidayRule::WeekdayBefore { month, day, weekday } => {
                let mut date = NaiveDate::from_ymd_opt(year, *month, *day)?.pred_opt()?;
                while date.weekday() != weekday.to_chrono() {
                    date = date.pred_opt()?;
                }
                Some(date)
            }
            HolidayRule::WeekdayAfter { month, day, weekday } => {
                let mut date = NaiveDate::from_ymd_opt(year, *month, *day)?.succ_opt()?;
                while date.weekday() != weekday.to_chrono() {
                    date = date.succ_opt()?;
                }
                Some(date)
            }
        }
    }
}

fn nth_weekday(year: i32, month: u32, weekday: chrono::Weekday, n: i32) -> Option<NaiveDate> {
    if n > 0 {
        NaiveDate::from_weekday_of_month_opt(year, month, weekday, n as u8)
    } else if n < 0 {
        let first_next = if month == 12 {
            NaiveDate::from_ymd_opt(year + 1, 1, 1)?
        } else {
            NaiveDate::from_ymd_opt(year, month + 1, 1)?
        };
        let mut date = first_next.pred_opt()?;
        while date.weekday() != weekday {
            date = date.pred_opt()?;
        }
        let date = date - Duration::weeks((-n - 1) as i64);
        (date.month() == month).then_some(date)
    } else {
        None
    }
}

/// A holiday on a concrete date.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Holiday {
    pub id: String,
    pub date: NaiveDate,
    pub name: LocalizedText,
    pub day_off: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn rules_compute_dates() {
        assert_eq!(
            HolidayRule::Fixed("10-26".into()).date(2026),
            Some(d(2026, 10, 26))
        );
        assert_eq!(HolidayRule::Fixed("02-30".into()).date(2026), None);
        assert_eq!(HolidayRule::EasterOffset(-2).date(2026), Some(d(2026, 4, 3)));
        let mothers_day = HolidayRule::NthWeekday {
            month: 5,
            weekday: Weekday::Sun,
            n: 2,
        };
        assert_eq!(mothers_day.date(2026), Some(d(2026, 5, 10)));
        let last_monday_may = HolidayRule::NthWeekday {
            month: 5,
            weekday: Weekday::Mon,
            n: -1,
        };
        assert_eq!(last_monday_may.date(2026), Some(d(2026, 5, 25)));
        let buss_und_bettag = HolidayRule::WeekdayBefore {
            month: 11,
            day: 23,
            weekday: Weekday::Wed,
        };
        assert_eq!(buss_und_bettag.date(2025), Some(d(2025, 11, 19)));
        assert_eq!(buss_und_bettag.date(2026), Some(d(2026, 11, 18)));
        // 23 Nov 2022 was a Wednesday: the holiday is the week before.
        assert_eq!(buss_und_bettag.date(2022), Some(d(2022, 11, 16)));
    }
}
