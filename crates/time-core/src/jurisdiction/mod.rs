//! Country profiles: legal work-time rules, breaks, vacation defaults and
//! public holidays as data (docs/SPEC.md §5.13, docs/country-profiles.md).
//!
//! Profiles are TOML files in `crates/time-core/jurisdictions/`, embedded at
//! build time. Adding a country means adding a file and its fixtures; no code
//! changes.

mod easter;
mod holidays;

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

pub use holidays::{Holiday, HolidayRule};

/// Profiles bundled with the app: (id, TOML source).
const BUNDLED: &[(&str, &str)] = &[
    ("AT", include_str!("../../jurisdictions/AT.toml")),
    ("DE", include_str!("../../jurisdictions/DE.toml")),
    ("CH", include_str!("../../jurisdictions/CH.toml")),
];

/// Text in several languages, keyed by language code (`de`, `en`, …).
pub type LocalizedText = BTreeMap<String, String>;

/// Picks `lang`, then English, then any translation.
pub fn localize(text: &LocalizedText, lang: &str) -> String {
    text.get(lang)
        .or_else(|| text.get("en"))
        .or_else(|| text.values().next())
        .cloned()
        .unwrap_or_default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Weekday {
    pub fn to_chrono(self) -> chrono::Weekday {
        match self {
            Weekday::Mon => chrono::Weekday::Mon,
            Weekday::Tue => chrono::Weekday::Tue,
            Weekday::Wed => chrono::Weekday::Wed,
            Weekday::Thu => chrono::Weekday::Thu,
            Weekday::Fri => chrono::Weekday::Fri,
            Weekday::Sat => chrono::Weekday::Sat,
            Weekday::Sun => chrono::Weekday::Sun,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum RuleMode {
    /// Legal requirement; users may only make it stricter.
    Required,
    /// Recommendation; users may change it freely.
    Advisory,
}

/// Legal limits. `None` = no limit in this jurisdiction.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct WorkTimeRules {
    pub max_daily_minutes: Option<u32>,
    pub max_weekly_minutes: Option<u32>,
    pub min_daily_rest_minutes: Option<u32>,
    /// Default full-time week for new schedules.
    pub standard_weekly_minutes: u32,
    /// Default working days for new schedules.
    pub standard_workdays: Vec<Weekday>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct BreakRule {
    /// Applies when more than this was worked in a day.
    pub after_minutes: u32,
    /// Total break required.
    pub break_minutes: u32,
    /// Smallest gap that counts towards the break (the break may be split
    /// into parts of at least this length).
    pub min_part_minutes: u32,
    pub mode: RuleMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub enum VacationUnit {
    /// Days based on a 5-day week.
    Workdays,
    Hours,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(deny_unknown_fields)]
pub struct VacationRules {
    /// Statutory minimum per year (full-time, 5-day week).
    pub default_days: u32,
    pub unit: VacationUnit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HolidayDef {
    /// Stable id, used by regions to remove a holiday (e.g. `epiphany`).
    pub id: String,
    pub name: LocalizedText,
    pub rule: HolidayRule,
    /// `false` = informational only, not a day off.
    #[serde(default = "default_true")]
    pub day_off: bool,
    pub valid_from_year: Option<i32>,
    pub valid_to_year: Option<i32>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegionDef {
    pub name: LocalizedText,
    /// Holidays only in this region.
    #[serde(default)]
    pub holidays: Vec<HolidayDef>,
    /// Ids of country-wide holidays that do not apply in this region.
    #[serde(default)]
    pub remove_holidays: Vec<String>,
}

/// A profile file as written in TOML.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProfileDef {
    pub schema_version: u32,
    /// ISO 3166-1 alpha-2 code.
    pub id: String,
    /// Bump when rules change; computed days record the version they used.
    pub version: String,
    pub name: LocalizedText,
    pub maintainer: String,
    /// Links to the law texts the values come from.
    pub sources: Vec<String>,
    pub default_locale: String,
    pub week_start: Weekday,
    pub work_time: WorkTimeRules,
    /// Required breaks; the rule with the highest matching `after_minutes` applies.
    pub breaks: Vec<BreakRule>,
    pub vacation: VacationRules,
    pub holidays: Vec<HolidayDef>,
    /// Regions by ISO 3166-2 code (e.g. `DE-BY`).
    #[serde(default)]
    pub regions: BTreeMap<String, RegionDef>,
}

pub const SCHEMA_VERSION: u32 = 1;

/// A profile with its region applied, ready to use.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub id: String,
    pub version: String,
    pub region: Option<String>,
    pub name: LocalizedText,
    pub region_name: Option<LocalizedText>,
    pub week_start: Weekday,
    pub work_time: WorkTimeRules,
    pub breaks: Vec<BreakRule>,
    pub vacation: VacationRules,
    holidays: Vec<HolidayDef>,
}

impl Profile {
    /// Holidays in `year`, sorted by date. Includes informational ones
    /// (`day_off = false`).
    pub fn holidays(&self, year: i32) -> Vec<Holiday> {
        let mut list: Vec<Holiday> = self
            .holidays
            .iter()
            .filter(|h| h.valid_from_year.is_none_or(|y| year >= y))
            .filter(|h| h.valid_to_year.is_none_or(|y| year <= y))
            .filter_map(|h| {
                h.rule.date(year).map(|date| Holiday {
                    id: h.id.clone(),
                    date,
                    name: h.name.clone(),
                    day_off: h.day_off,
                })
            })
            .collect();
        list.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.id.cmp(&b.id)));
        list
    }

    /// Required break for a day with `worked_minutes` of work.
    pub fn required_break(&self, worked_minutes: u32) -> Option<&BreakRule> {
        self.breaks
            .iter()
            .filter(|b| worked_minutes > b.after_minutes)
            .max_by_key(|b| b.after_minutes)
    }
}

/// Summary of a bundled profile for pickers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ProfileInfo {
    pub id: String,
    pub version: String,
    pub name: LocalizedText,
    pub default_locale: String,
    pub regions: Vec<RegionInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RegionInfo {
    pub id: String,
    pub name: LocalizedText,
}

pub fn parse(source: &str) -> Result<ProfileDef> {
    let def: ProfileDef = toml::from_str(source).map_err(|e| Error::validation("profile", e.to_string()))?;
    if def.schema_version != SCHEMA_VERSION {
        return Err(Error::validation(
            "profile",
            format!("{}: unsupported schema_version {}", def.id, def.schema_version),
        ));
    }
    Ok(def)
}

/// All bundled profiles. Panics only if a bundled file is invalid, which the
/// test suite prevents.
pub fn bundled() -> Vec<ProfileDef> {
    BUNDLED
        .iter()
        .map(|(id, src)| parse(src).unwrap_or_else(|e| panic!("bundled profile {id} is invalid: {e}")))
        .collect()
}

pub fn list() -> Vec<ProfileInfo> {
    bundled()
        .into_iter()
        .map(|p| ProfileInfo {
            id: p.id,
            version: p.version,
            name: p.name,
            default_locale: p.default_locale,
            regions: p
                .regions
                .into_iter()
                .map(|(id, r)| RegionInfo { id, name: r.name })
                .collect(),
        })
        .collect()
}

/// Loads a bundled profile and applies `region`.
pub fn load(id: &str, region: Option<&str>) -> Result<Profile> {
    let def = bundled()
        .into_iter()
        .find(|p| p.id.eq_ignore_ascii_case(id))
        .ok_or_else(|| Error::validation("profileId", format!("unknown country profile '{id}'")))?;
    resolve(def, region)
}

pub fn resolve(def: ProfileDef, region: Option<&str>) -> Result<Profile> {
    let mut holidays = def.holidays;
    let mut region_name = None;
    if let Some(code) = region {
        let r = def
            .regions
            .get(code)
            .ok_or_else(|| Error::validation("region", format!("unknown region '{code}' for {}", def.id)))?;
        holidays.retain(|h| !r.remove_holidays.contains(&h.id));
        holidays.extend(r.holidays.iter().cloned());
        region_name = Some(r.name.clone());
    }
    Ok(Profile {
        id: def.id,
        version: def.version,
        region: region.map(str::to_string),
        name: def.name,
        region_name,
        week_start: def.week_start,
        work_time: def.work_time,
        breaks: def.breaks,
        vacation: def.vacation,
        holidays,
    })
}

/// Suggests a profile for an OS locale like `de-AT` or `de_DE.UTF-8`.
pub fn suggest_for_locale(locale: &str) -> Option<String> {
    let country = locale
        .split(['-', '_', '.'])
        .nth(1)
        .map(|c| c.to_ascii_uppercase())?;
    bundled().into_iter().find(|p| p.id == country).map(|p| p.id)
}

/// JSON Schema of the profile format (`jurisdictions/profile.schema.json`).
pub fn json_schema() -> String {
    let schema = schemars::schema_for!(ProfileDef);
    serde_json::to_string_pretty(&schema).expect("schema serializes") + "\n"
}

#[cfg(test)]
mod tests;
