//! Regional formats for displayed and exported values: dates, times,
//! decimal numbers, first day of the week, and the effective time zone.
//!
//! Stored data never depends on these: times stay unix ms (UTC), dates
//! ISO `YYYY-MM-DD`. Only what is shown or exported is formatted.

use chrono::{Datelike, NaiveDate, NaiveTime, Offset, TimeZone, Timelike, Weekday};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum DateFormat {
    /// From the operating system's regional settings.
    #[default]
    Auto,
    /// `31.12.2026`
    Dmy,
    /// `12/31/2026`
    Mdy,
    /// `2026-12-31` (ISO 8601)
    Ymd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum TimeFormat {
    /// From the operating system's regional settings.
    #[default]
    Auto,
    /// `14:30`
    H24,
    /// `2:30 PM`
    H12,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum NumberFormat {
    /// From the operating system's regional settings.
    #[default]
    Auto,
    /// `7.5`
    Point,
    /// `7,5`
    Comma,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum WeekStart {
    /// From the operating system's regional settings.
    #[default]
    Auto,
    Monday,
    Sunday,
    Saturday,
}

impl WeekStart {
    /// `Auto` counts as Monday; resolve it first (see [`system_formats`]).
    pub fn weekday(self) -> Weekday {
        match self {
            Self::Monday | Self::Auto => Weekday::Mon,
            Self::Sunday => Weekday::Sun,
            Self::Saturday => Weekday::Sat,
        }
    }

    /// First day of the week containing `date`.
    pub fn week_of(self, date: NaiveDate) -> NaiveDate {
        let back = (7 + date.weekday().num_days_from_monday() - self.weekday().num_days_from_monday()) % 7;
        date - chrono::Duration::days(back.into())
    }
}

/// Formats of a system locale (`de-AT`, `en_US.UTF-8`, `fr`), never `Auto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RegionalFormats {
    pub date: DateFormat,
    pub time: TimeFormat,
    pub number: NumberFormat,
    pub week_start: WeekStart,
}

/// Regional conventions by locale. Covers the common cases: month-first
/// dates and 12-hour clocks where they are the norm, decimal commas by
/// language, and the usual first day of the week by region.
pub fn system_formats(locale: &str) -> RegionalFormats {
    let normalized = locale.split(['.', '@']).next().unwrap_or("").replace('_', "-");
    let mut parts = normalized.split('-');
    let lang = parts.next().unwrap_or("").to_ascii_lowercase();
    // The region is the 2-letter (or 3-digit) part; skip scripts like `Hant`.
    let region = parts
        .find(|p| p.len() == 2 || (p.len() == 3 && p.chars().all(|c| c.is_ascii_digit())))
        .map(str::to_ascii_uppercase)
        .unwrap_or_else(|| match lang.as_str() {
            // Bare language: its most common region.
            "en" => "US".into(),
            "de" => "DE".into(),
            other => other.to_ascii_uppercase(),
        });
    let r = region.as_str();

    let date = match r {
        "US" | "PH" | "FM" | "MH" | "PW" => DateFormat::Mdy,
        "CN" | "JP" | "KR" | "TW" | "HU" | "LT" | "SE" | "MN" | "IR" => DateFormat::Ymd,
        "CA" if lang == "en" => DateFormat::Ymd,
        _ => DateFormat::Dmy,
    };
    let time = match r {
        "US" | "CA" | "AU" | "NZ" | "IN" | "PH" | "PK" | "BD" | "EG" | "SA" | "MY" | "CO" | "SV" | "HN"
        | "NI" | "JO" | "IQ" | "SY" | "LY" | "OM" | "QA" | "KW" | "BH" | "AE"
            if lang != "fr" =>
        {
            TimeFormat::H12
        }
        _ => TimeFormat::H24,
    };
    const COMMA_LANGS: &[&str] = &[
        "de", "fr", "es", "it", "nl", "pt", "ru", "pl", "cs", "sk", "sv", "da", "nb", "nn", "no", "fi", "tr",
        "hu", "ro", "bg", "hr", "sl", "sr", "uk", "el", "id", "vi", "lt", "lv", "et", "ca", "be", "is", "kk",
        "az",
    ];
    let point_regions = ["CH", "LI", "MX", "PR", "GT", "DO", "HN", "NI", "SV", "PA", "PE"];
    let number = if COMMA_LANGS.contains(&lang.as_str()) && !point_regions.contains(&r) {
        NumberFormat::Comma
    } else {
        NumberFormat::Point
    };
    let week_start = match r {
        "US" | "CA" | "MX" | "BR" | "JP" | "IL" | "PH" | "ZA" | "KR" | "TW" | "HK" | "SA" | "IN" | "PE"
        | "CO" | "GT" | "HN" | "SV" | "NI" | "PA" | "DO" | "PR" | "VE" | "BO" | "PY" | "TH" | "KE" | "ZW"
        | "BZ" => WeekStart::Sunday,
        "EG" | "IR" | "AF" | "DZ" | "IQ" | "JO" | "KW" | "LY" | "OM" | "QA" | "SY" | "BH" | "SD" => {
            WeekStart::Saturday
        }
        _ => WeekStart::Monday,
    };
    RegionalFormats {
        date,
        time,
        number,
        week_start,
    }
}

/// Formats with `Auto` resolved from the system locale; texts (column
/// headers, month names) in the UI language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Formats {
    pub german: bool,
    pub date: DateFormat,
    pub time: TimeFormat,
    pub number: NumberFormat,
    pub week_start: WeekStart,
}

impl Formats {
    /// `lang` is the UI language (`"de"` or `"en"`), `system` the resolved
    /// system formats that `Auto` falls back to.
    pub fn new(
        lang: &str,
        system: RegionalFormats,
        date: DateFormat,
        time: TimeFormat,
        number: NumberFormat,
        week_start: WeekStart,
    ) -> Self {
        fn pick<T: PartialEq>(v: T, auto: T, sys: T) -> T {
            if v == auto {
                sys
            } else {
                v
            }
        }
        Self {
            german: lang == "de",
            date: pick(date, DateFormat::Auto, system.date),
            time: pick(time, TimeFormat::Auto, system.time),
            number: pick(number, NumberFormat::Auto, system.number),
            week_start: pick(week_start, WeekStart::Auto, system.week_start),
        }
    }

    pub fn lang(&self) -> &'static str {
        if self.german {
            "de"
        } else {
            "en"
        }
    }

    pub fn decimal_comma(&self) -> bool {
        self.number == NumberFormat::Comma
    }

    pub fn date(&self, d: NaiveDate) -> String {
        match self.date {
            DateFormat::Dmy | DateFormat::Auto => d.format("%d.%m.%Y").to_string(),
            DateFormat::Mdy => d.format("%m/%d/%Y").to_string(),
            DateFormat::Ymd => d.format("%Y-%m-%d").to_string(),
        }
    }

    /// Excel number format for date cells.
    pub fn excel_date(&self) -> &'static str {
        match self.date {
            DateFormat::Dmy | DateFormat::Auto => "dd.mm.yyyy",
            DateFormat::Mdy => "mm/dd/yyyy",
            DateFormat::Ymd => "yyyy-mm-dd",
        }
    }

    pub fn time(&self, t: NaiveTime) -> String {
        match self.time {
            TimeFormat::H12 => {
                let (pm, h) = t.hour12();
                format!("{h}:{:02} {}", t.minute(), if pm { "PM" } else { "AM" })
            }
            _ => t.format("%H:%M").to_string(),
        }
    }

    /// Fixed decimals with the configured separator; no grouping, so the
    /// value stays readable by spreadsheet imports.
    pub fn decimal(&self, value: f64, decimals: usize) -> String {
        let s = format!("{value:.decimals$}");
        if self.decimal_comma() {
            s.replace('.', ",")
        } else {
            s
        }
    }

    pub fn month_year(&self, year: i32, month: u32) -> String {
        const EN: [&str; 12] = [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ];
        const DE: [&str; 12] = [
            "Januar",
            "Februar",
            "März",
            "April",
            "Mai",
            "Juni",
            "Juli",
            "August",
            "September",
            "Oktober",
            "November",
            "Dezember",
        ];
        let names = if self.german { DE } else { EN };
        let name = names.get(month.saturating_sub(1) as usize).copied().unwrap_or("");
        format!("{name} {year}")
    }

    /// Label of the week starting on `start`: ISO week number for Monday
    /// weeks, otherwise "week of <date>".
    pub fn week(&self, start: NaiveDate) -> String {
        match (self.week_start, self.german) {
            (WeekStart::Monday, true) => {
                format!("KW {} {}", start.iso_week().week(), start.iso_week().year())
            }
            (WeekStart::Monday, false) => {
                format!("Week {} {}", start.iso_week().week(), start.iso_week().year())
            }
            (_, true) => format!("Woche ab {}", self.date(start)),
            (_, false) => format!("Week of {}", self.date(start)),
        }
    }
}

/// The zone to use for `zone`, optionally without daylight saving time:
/// the zone's standard offset as a fixed `Etc/GMT±N` zone. Zones without
/// DST, and those whose standard offset is not a whole hour (no `Etc`
/// zone exists for them), are returned unchanged.
pub fn effective_zone(zone: &str, daylight_saving: bool, year: i32) -> String {
    if daylight_saving {
        return zone.to_string();
    }
    let Ok(tz) = zone.parse::<chrono_tz::Tz>() else {
        return zone.to_string();
    };
    let offset = |month| {
        tz.with_ymd_and_hms(year, month, 1, 12, 0, 0)
            .single()
            .map(|d| d.offset().fix().local_minus_utc())
    };
    let (Some(jan), Some(jul)) = (offset(1), offset(7)) else {
        return zone.to_string();
    };
    // DST moves clocks forward, so the standard offset is the smaller one.
    let standard = jan.min(jul);
    if jan == jul || standard % 3600 != 0 {
        return zone.to_string();
    }
    let hours = standard / 3600;
    // `Etc` zones have inverted signs: UTC+1 is `Etc/GMT-1`.
    match hours {
        0 => "Etc/GMT".into(),
        h if h > 0 => format!("Etc/GMT-{h}"),
        h => format!("Etc/GMT+{}", -h),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn auto(locale: &str) -> Formats {
        Formats::new(
            "en",
            system_formats(locale),
            DateFormat::Auto,
            TimeFormat::Auto,
            NumberFormat::Auto,
            WeekStart::Auto,
        )
    }

    #[test]
    fn auto_follows_the_system_locale() {
        let t = NaiveTime::from_hms_opt(14, 5, 0).unwrap();
        let at = auto("de-AT");
        assert_eq!(at.date(d("2026-12-31")), "31.12.2026");
        assert_eq!(at.time(t), "14:05");
        assert_eq!(at.decimal(7.5, 2), "7,50");
        assert_eq!(at.week_start, WeekStart::Monday);

        let us = auto("en_US.UTF-8");
        assert_eq!(us.date(d("2026-12-31")), "12/31/2026");
        assert_eq!(us.time(t), "2:05 PM");
        assert_eq!(us.decimal(7.5, 2), "7.50");
        assert_eq!(us.week_start, WeekStart::Sunday);

        let gb = auto("en-GB");
        assert_eq!(gb.date(d("2026-12-31")), "31.12.2026");
        assert_eq!(gb.time(t), "14:05");
        assert_eq!(gb.number, NumberFormat::Point);

        assert_eq!(auto("de-CH").number, NumberFormat::Point);
        assert_eq!(auto("ja-JP").date, DateFormat::Ymd);
        assert_eq!(auto("zh-Hant-TW").date, DateFormat::Ymd);
        assert_eq!(auto("ar-EG").week_start, WeekStart::Saturday);
        assert_eq!(auto("fr-CA").time, TimeFormat::H24);
        assert_eq!(auto("de").number, NumberFormat::Comma);
        assert_eq!(auto("").date, DateFormat::Dmy);
    }

    #[test]
    fn explicit_formats_override_the_system() {
        let f = Formats::new(
            "de",
            system_formats("de-AT"),
            DateFormat::Ymd,
            TimeFormat::H12,
            NumberFormat::Point,
            WeekStart::Sunday,
        );
        assert_eq!(f.date(d("2026-01-02")), "2026-01-02");
        assert_eq!(f.time(NaiveTime::from_hms_opt(0, 30, 0).unwrap()), "12:30 AM");
        assert_eq!(f.decimal(1.25, 1), "1.2");
        assert_eq!(f.excel_date(), "yyyy-mm-dd");
    }

    #[test]
    fn weeks_start_on_the_configured_day() {
        let thu = d("2026-10-08");
        assert_eq!(WeekStart::Monday.week_of(thu), d("2026-10-05"));
        assert_eq!(WeekStart::Sunday.week_of(thu), d("2026-10-04"));
        assert_eq!(WeekStart::Saturday.week_of(thu), d("2026-10-03"));
        assert_eq!(WeekStart::Sunday.week_of(d("2026-10-04")), d("2026-10-04"));
        let de = Formats::new(
            "de",
            system_formats("de-DE"),
            DateFormat::Auto,
            TimeFormat::Auto,
            NumberFormat::Auto,
            WeekStart::Monday,
        );
        assert_eq!(de.week(d("2026-10-05")), "KW 41 2026");
    }

    #[test]
    fn without_dst_the_standard_offset_is_used() {
        assert_eq!(effective_zone("Europe/Vienna", true, 2026), "Europe/Vienna");
        assert_eq!(effective_zone("Europe/Vienna", false, 2026), "Etc/GMT-1");
        assert_eq!(effective_zone("America/New_York", false, 2026), "Etc/GMT+5");
        assert_eq!(effective_zone("Australia/Sydney", false, 2026), "Etc/GMT-10");
        assert_eq!(effective_zone("Europe/London", false, 2026), "Etc/GMT");
        // No DST, or no whole-hour standard offset: unchanged.
        assert_eq!(effective_zone("Asia/Tokyo", false, 2026), "Asia/Tokyo");
        assert_eq!(
            effective_zone("America/St_Johns", false, 2026),
            "America/St_Johns"
        );
    }
}
