//! Easter Sunday dates, used for movable holidays.

use chrono::NaiveDate;

/// Western (Gregorian) Easter Sunday. Anonymous Gregorian algorithm
/// (Meeus/Jones/Butcher); valid for all Gregorian years.
pub fn western(year: i32) -> NaiveDate {
    let a = year % 19;
    let b = year / 100;
    let c = year % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    NaiveDate::from_ymd_opt(year, month as u32, day as u32).expect("valid easter date")
}

/// Orthodox Easter Sunday as a Gregorian date (Meeus Julian algorithm plus
/// the Julian→Gregorian offset, 13 days for 1900–2099).
pub fn orthodox(year: i32) -> NaiveDate {
    let a = year % 4;
    let b = year % 7;
    let c = year % 19;
    let d = (19 * c + 15) % 30;
    let e = (2 * a + 4 * b - d + 34) % 7;
    let month = (d + e + 114) / 31;
    let day = (d + e + 114) % 31 + 1;
    let julian = NaiveDate::from_ymd_opt(year, month as u32, day as u32).expect("valid easter date");
    let offset = year / 100 - year / 400 - 2;
    julian + chrono::Duration::days(offset as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn known_western_dates() {
        assert_eq!(western(2024), d(2024, 3, 31));
        assert_eq!(western(2025), d(2025, 4, 20));
        assert_eq!(western(2026), d(2026, 4, 5));
        assert_eq!(western(2027), d(2027, 3, 28));
        assert_eq!(western(2028), d(2028, 4, 16));
        assert_eq!(western(2038), d(2038, 4, 25));
    }

    #[test]
    fn known_orthodox_dates() {
        assert_eq!(orthodox(2024), d(2024, 5, 5));
        assert_eq!(orthodox(2025), d(2025, 4, 20));
        assert_eq!(orthodox(2026), d(2026, 4, 12));
        assert_eq!(orthodox(2027), d(2027, 5, 2));
    }
}
