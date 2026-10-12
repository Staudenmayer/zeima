use chrono::NaiveDate;
use serde::Deserialize;

use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixtures {
    holidays: Vec<HolidayFixture>,
    breaks: Vec<BreakFixture>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HolidayFixture {
    year: i32,
    region: Option<String>,
    dates: Vec<NaiveDate>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BreakFixture {
    worked_minutes: u32,
    required_minutes: u32,
}

const FIXTURES: &[(&str, &str)] = &[
    ("AT", include_str!("../../jurisdictions/AT.fixtures.toml")),
    ("DE", include_str!("../../jurisdictions/DE.fixtures.toml")),
    ("CH", include_str!("../../jurisdictions/CH.fixtures.toml")),
];

#[test]
fn every_bundled_profile_has_fixtures_and_matches_them() {
    let profiles = bundled();
    assert_eq!(
        profiles.len(),
        FIXTURES.len(),
        "each profile needs a fixtures file"
    );
    for (id, src) in FIXTURES {
        let fixtures: Fixtures = toml::from_str(src).unwrap_or_else(|e| panic!("{id} fixtures: {e}"));
        assert!(
            !fixtures.holidays.is_empty() && !fixtures.breaks.is_empty(),
            "{id}: empty fixtures"
        );
        for f in &fixtures.holidays {
            let profile = load(id, f.region.as_deref()).unwrap();
            let actual: Vec<NaiveDate> = profile
                .holidays(f.year)
                .into_iter()
                .filter(|h| h.day_off)
                .map(|h| h.date)
                .collect();
            assert_eq!(actual, f.dates, "{id} {:?} {}", f.region, f.year);
        }
        let profile = load(id, None).unwrap();
        for f in &fixtures.breaks {
            let required = profile
                .required_break(f.worked_minutes)
                .map_or(0, |b| b.break_minutes);
            assert_eq!(
                required, f.required_minutes,
                "{id} break after {} min",
                f.worked_minutes
            );
        }
    }
}

#[test]
fn every_region_resolves_and_profiles_are_consistent() {
    for def in bundled() {
        assert_eq!(def.id.len(), 2, "{}: id must be ISO 3166-1 alpha-2", def.id);
        assert!(def.name.contains_key("en"), "{}: needs an English name", def.id);
        assert!(!def.sources.is_empty(), "{}: needs sources", def.id);
        let mut ids: Vec<_> = def.holidays.iter().map(|h| &h.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), def.holidays.len(), "{}: duplicate holiday ids", def.id);
        for (code, region) in &def.regions {
            assert!(
                code.starts_with(&format!("{}-", def.id)),
                "{code}: must be ISO 3166-2 of {}",
                def.id
            );
            for removed in &region.remove_holidays {
                assert!(
                    def.holidays.iter().any(|h| &h.id == removed),
                    "{code}: removes unknown {removed}"
                );
            }
            let profile = load(&def.id, Some(code)).unwrap();
            for year in 2020..2040 {
                let days: Vec<_> = profile
                    .holidays(year)
                    .into_iter()
                    .map(|h| (h.date, h.id))
                    .collect();
                let mut dedup = days.clone();
                dedup.dedup_by(|a, b| a.1 == b.1);
                assert_eq!(days.len(), dedup.len(), "{code} {year}: holiday twice");
            }
        }
    }
}

#[test]
fn unknown_profile_or_region_is_an_error() {
    assert!(load("XX", None).is_err());
    assert!(load("AT", Some("DE-BY")).is_err());
    assert!(load("at", None).is_ok(), "ids are case-insensitive");
}

#[test]
fn locale_suggests_profile() {
    assert_eq!(suggest_for_locale("de-AT").as_deref(), Some("AT"));
    assert_eq!(suggest_for_locale("de_DE.UTF-8").as_deref(), Some("DE"));
    assert_eq!(suggest_for_locale("en-US"), None);
    assert_eq!(suggest_for_locale("de"), None);
}

#[test]
fn unknown_fields_are_rejected() {
    let src = include_str!("../../jurisdictions/AT.toml")
        .replace("schema_version = 1", "schema_version = 1\ntypo = 1");
    assert!(parse(&src).is_err());
}

/// Keeps `jurisdictions/profile.schema.json` in sync with the Rust types;
/// CI fails if the committed file differs.
#[test]
fn json_schema_is_up_to_date() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/jurisdictions/profile.schema.json");
    std::fs::write(path, json_schema()).unwrap();
}
