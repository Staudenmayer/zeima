//! Device-local settings (shortcuts, autostart, display, regional
//! formats). Stored in `local_meta`, never synced: they belong to a machine.

use chrono::Datelike;
use serde::{Deserialize, Serialize};
use specta::Type;
use time_core::locale::{
    effective_zone, system_formats, DateFormat, Formats, NumberFormat, RegionalFormats, TimeFormat, WeekStart,
};
use time_core::Core;

const KEY: &str = "settings";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DurationFormat {
    /// `7:42 h`
    Hm,
    /// `7.70 h`
    Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub shortcut_toggle_window: String,
    pub shortcut_quick_switch: String,
    pub shortcut_pause_resume: String,
    pub autostart: bool,
    /// `None` follows the operating system.
    pub locale: Option<String>,
    pub duration_format: DurationFormat,
    /// IANA zone; `None` follows the operating system.
    pub timezone: Option<String>,
    /// `false` keeps the zone's standard time all year.
    pub daylight_saving: bool,
    pub date_format: DateFormat,
    pub time_format: TimeFormat,
    pub number_format: NumberFormat,
    pub week_start: WeekStart,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            shortcut_toggle_window: "CommandOrControl+Shift+T".into(),
            shortcut_quick_switch: "CommandOrControl+Shift+Space".into(),
            shortcut_pause_resume: "CommandOrControl+Shift+P".into(),
            autostart: false,
            locale: None,
            duration_format: DurationFormat::Hm,
            timezone: None,
            daylight_saving: true,
            date_format: DateFormat::Auto,
            time_format: TimeFormat::Auto,
            number_format: NumberFormat::Auto,
            week_start: WeekStart::Auto,
        }
    }
}

impl AppSettings {
    /// Language for texts rendered by Rust (tray menu): "de" or "en".
    pub fn language(&self) -> &'static str {
        let locale = self
            .locale
            .clone()
            .or_else(sys_locale::get_locale)
            .unwrap_or_default();
        if locale.to_lowercase().starts_with("de") {
            "de"
        } else {
            "en"
        }
    }
}

/// The operating system's IANA zone, or UTC if it cannot be read.
pub fn system_timezone() -> String {
    iana_time_zone::get_timezone()
        .ok()
        .filter(|tz| time_core::model::validate_tz(tz).is_ok())
        .unwrap_or_else(|| "UTC".into())
}

impl AppSettings {
    /// Zone used for day boundaries, display and exports.
    pub fn effective_timezone(&self) -> String {
        let base = self
            .timezone
            .clone()
            .filter(|tz| time_core::model::validate_tz(tz).is_ok())
            .unwrap_or_else(system_timezone);
        effective_zone(&base, self.daylight_saving, chrono::Utc::now().year())
    }

    /// Regional formats with "automatic" resolved from the system locale.
    pub fn formats(&self) -> Formats {
        Formats::new(
            self.language(),
            system_regional_formats(),
            self.date_format,
            self.time_format,
            self.number_format,
            self.week_start,
        )
    }
}

/// The operating system's locale, e.g. `de-AT`.
pub fn system_locale() -> String {
    sys_locale::get_locale().unwrap_or_else(|| "en-US".into())
}

/// Formats of the operating system's locale ("automatic").
pub fn system_regional_formats() -> RegionalFormats {
    system_formats(&system_locale())
}

/// Applies time zone and week start to the core. Returns whether the zone
/// changed; cached days are then dropped because day boundaries moved.
pub async fn apply_regional(core: &Core, settings: &AppSettings) -> bool {
    core.set_week_start(settings.formats().week_start);
    let tz = settings.effective_timezone();
    if tz == core.default_tz() {
        return false;
    }
    if let Err(err) = core.set_default_tz(&tz) {
        eprintln!("ignoring time zone {tz}: {err}");
        return false;
    }
    if let Err(err) = core.clear_day_cache().await {
        eprintln!("could not clear the day cache: {err}");
    }
    true
}

/// Falls back to defaults if nothing (or something unreadable) is stored.
pub async fn load(core: &Core) -> AppSettings {
    match core.get_meta(KEY).await {
        Ok(Some(json)) => serde_json::from_str(&json).unwrap_or_default(),
        _ => AppSettings::default(),
    }
}

pub async fn save(core: &Core, settings: &AppSettings) -> time_core::Result<()> {
    core.set_meta(KEY, &serde_json::to_string(settings)?).await
}
