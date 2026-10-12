//! Domain models shared by the desktop app (and later the server).
//!
//! Serialized as camelCase. The same JSON is written to the change log, so it
//! is also the sync payload format.
//!
//! `i64` fields (unix ms, minutes) are exported to TypeScript as `number`:
//! JS numbers hold integers up to 2^53 exactly, far beyond these values.

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Declares a string-backed enum with stable storage values.
macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[cfg_attr(feature = "specta", derive(specta::Type))]
        pub enum $name {
            $(#[serde(rename = $value)] $variant),+
        }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $value),+ }
            }
        }

        impl std::str::FromStr for $name {
            type Err = Error;
            fn from_str(s: &str) -> Result<Self> {
                match s {
                    $($value => Ok(Self::$variant),)+
                    _ => Err(Error::validation(stringify!($name), format!("unknown value '{s}'"))),
                }
            }
        }
    };
}

string_enum!(BudgetPeriod { Total => "total", Monthly => "monthly", Weekly => "weekly" });
string_enum!(ProjectStatus { Active => "active", Paused => "paused", Archived => "archived" });
string_enum!(EntrySource {
    Timer => "timer",
    Manual => "manual",
    Calendar => "calendar",
    Meeting => "meeting",
    Import => "import",
    Addon => "addon",
});
string_enum!(PauseKind {
    Manual => "manual",
    AutoLunch => "auto_lunch",
    AutoIdle => "auto_idle",
    AutoNight => "auto_night",
    AutoLock => "auto_lock",
});

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    /// Short code / project number, unique per workspace (e.g. `P-2024-017`).
    pub alias: Option<String>,
    pub color: String,
    pub parent_id: Option<String>,
    pub client: Option<String>,
    /// Allotted time per `budget_period`.
    #[cfg_attr(feature = "specta", specta(type = Option<specta_typescript::Number>))]
    pub budget_minutes: Option<i64>,
    pub budget_period: BudgetPeriod,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub budget_warn_pct: i64,
    pub billable_default: bool,
    pub status: ProjectStatus,
    /// Default cost center for entries on this project.
    pub cost_center_id: Option<String>,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub created_at: i64,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ProjectInput {
    pub name: String,
    pub alias: Option<String>,
    pub color: String,
    pub parent_id: Option<String>,
    pub client: Option<String>,
    #[cfg_attr(feature = "specta", specta(type = Option<specta_typescript::Number>))]
    pub budget_minutes: Option<i64>,
    pub budget_period: BudgetPeriod,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub budget_warn_pct: i64,
    pub billable_default: bool,
    pub status: ProjectStatus,
    #[serde(default)]
    pub cost_center_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub color: String,
    /// `None` = global tag, otherwise only usable on this project.
    pub project_id: Option<String>,
    /// Tags in the same group are mutually exclusive on an entry.
    pub group_name: Option<String>,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub created_at: i64,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct TagInput {
    pub name: String,
    pub color: String,
    pub project_id: Option<String>,
    pub group_name: Option<String>,
}

/// Kostenstelle: accounting unit time is allocated to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct CostCenter {
    pub id: String,
    pub workspace_id: String,
    /// Accounting code, unique per workspace (e.g. `4711`).
    pub code: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub active: bool,
    /// Inclusive validity; `None` = open-ended.
    pub valid_from: Option<chrono::NaiveDate>,
    pub valid_to: Option<chrono::NaiveDate>,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub created_at: i64,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub updated_at: i64,
}

impl CostCenter {
    /// Active and valid on `date`.
    pub fn usable_on(&self, date: chrono::NaiveDate) -> bool {
        self.active
            && self.valid_from.is_none_or(|from| from <= date)
            && self.valid_to.is_none_or(|to| date <= to)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct CostCenterInput {
    pub code: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub active: bool,
    pub valid_from: Option<chrono::NaiveDate>,
    pub valid_to: Option<chrono::NaiveDate>,
}

/// Default cost center of the user from `valid_from` on (inclusive range).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct CostCenterAssignment {
    pub id: String,
    pub cost_center_id: String,
    pub valid_from: chrono::NaiveDate,
    pub valid_to: Option<chrono::NaiveDate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct CostCenterAssignmentInput {
    pub cost_center_id: String,
    pub valid_from: chrono::NaiveDate,
    pub valid_to: Option<chrono::NaiveDate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct TimeEntry {
    pub id: String,
    pub workspace_id: String,
    pub user_id: String,
    pub project_id: Option<String>,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub start_at: i64,
    /// `None` while the timer is running.
    #[cfg_attr(feature = "specta", specta(type = Option<specta_typescript::Number>))]
    pub end_at: Option<i64>,
    /// IANA time zone at start.
    pub tz: String,
    pub note: String,
    pub billable: bool,
    pub source: EntrySource,
    pub source_ref: Option<String>,
    pub locked: bool,
    pub tag_ids: Vec<String>,
    /// Resolved when the entry is written (see `Core::resolve_cost_center`).
    pub cost_center_id: Option<String>,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub created_at: i64,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub updated_at: i64,
}

impl TimeEntry {
    pub fn is_running(&self) -> bool {
        self.end_at.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct EntryInput {
    pub project_id: Option<String>,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub start_at: i64,
    #[cfg_attr(feature = "specta", specta(type = Option<specta_typescript::Number>))]
    pub end_at: Option<i64>,
    pub tz: Option<String>,
    pub note: String,
    /// `None` = take the project's default.
    pub billable: Option<bool>,
    pub tag_ids: Vec<String>,
    /// `None` = resolve from the project, then the user's default.
    #[serde(default)]
    pub cost_center_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Pause {
    pub id: String,
    pub workspace_id: String,
    pub user_id: String,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub start_at: i64,
    #[cfg_attr(feature = "specta", specta(type = Option<specta_typescript::Number>))]
    pub end_at: Option<i64>,
    pub kind: PauseKind,
    /// Entry that was running when the pause started; resume continues it.
    pub resume_entry_id: Option<String>,
    /// Auto-pause: entry that continues after the pause.
    pub followup_entry_id: Option<String>,
    /// Auto-pause: the pause has been cut out of the entries.
    pub applied: bool,
    /// Auto-pause: `None` while waiting for the user's decision.
    pub accepted: Option<bool>,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub created_at: i64,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct StartTimer {
    pub project_id: Option<String>,
    pub tag_ids: Vec<String>,
    pub note: String,
    pub billable: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum TimerState {
    Idle,
    Running {
        entry: TimeEntry,
    },
    Paused {
        pause: Pause,
        #[serde(rename = "resumeEntry")]
        resume_entry: Option<TimeEntry>,
    },
}

string_enum!(HalfDay { Am => "am", Pm => "pm" });
string_enum!(AbsenceStatus { Requested => "requested", Approved => "approved", Rejected => "rejected" });

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct AbsenceType {
    pub id: String,
    /// Built-in types: `vacation`, `sick`, `comp_time`, `training`, `other`.
    pub key: Option<String>,
    pub name: String,
    pub color: String,
    /// Vacation, sickness etc. reduce the day's target. Comp time does not:
    /// it is taken from the balance.
    pub reduces_target: bool,
    /// Counts against the yearly vacation entitlement.
    pub affects_vacation: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct AbsenceTypeInput {
    pub name: String,
    pub color: String,
    pub reduces_target: bool,
    pub affects_vacation: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Absence {
    pub id: String,
    pub user_id: String,
    pub type_id: String,
    pub start_date: chrono::NaiveDate,
    /// Inclusive.
    pub end_date: chrono::NaiveDate,
    pub half_day: Option<HalfDay>,
    pub status: AbsenceStatus,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct AbsenceInput {
    pub type_id: String,
    pub start_date: chrono::NaiveDate,
    pub end_date: chrono::NaiveDate,
    pub half_day: Option<HalfDay>,
    pub note: String,
}

impl AbsenceInput {
    pub fn normalized(mut self) -> Result<Self> {
        if self.end_date < self.start_date {
            return Err(Error::validation("endDate", "must not be before the start"));
        }
        if self.half_day.is_some() && self.start_date != self.end_date {
            return Err(Error::validation("halfDay", "only possible for a single day"));
        }
        self.note = self.note.trim().to_string();
        validate_note(&self.note)?;
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct CustomHoliday {
    pub id: String,
    pub date: chrono::NaiveDate,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct BalanceAdjustment {
    pub id: String,
    pub date: chrono::NaiveDate,
    /// Positive adds to the balance, negative removes.
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub minutes: i64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct BalanceAdjustmentInput {
    pub date: chrono::NaiveDate,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub minutes: i64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct JurisdictionSelection {
    pub profile_id: String,
    pub region: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase", default)]
pub struct WorkSettings {
    /// `None` = the country profile's statutory minimum.
    pub vacation_days_per_year: Option<f64>,
    /// Deduct missing legal breaks from worked time (otherwise only warn).
    pub deduct_breaks: bool,
    pub auto_pause: crate::autopause::AutoPauseConfig,
}

impl Default for WorkSettings {
    fn default() -> Self {
        Self {
            vacation_days_per_year: None,
            deduct_breaks: true,
            auto_pause: crate::autopause::AutoPauseConfig::default(),
        }
    }
}

impl WorkSettings {
    pub fn validate(&self) -> Result<()> {
        if self
            .vacation_days_per_year
            .is_some_and(|d| !(0.0..=366.0).contains(&d))
        {
            return Err(Error::validation(
                "vacationDaysPerYear",
                "must be between 0 and 366",
            ));
        }
        self.auto_pause.validate()
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

pub const MAX_NAME_LEN: usize = 120;
pub const MAX_ALIAS_LEN: usize = 32;
pub const MAX_NOTE_LEN: usize = 2000;

fn trimmed_opt(v: &Option<String>) -> Option<String> {
    v.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn validate_name(field: &str, name: &str) -> Result<()> {
    let len = name.trim().chars().count();
    if len == 0 {
        return Err(Error::validation(field, "must not be empty"));
    }
    if len > MAX_NAME_LEN {
        return Err(Error::validation(
            field,
            format!("must be at most {MAX_NAME_LEN} characters"),
        ));
    }
    Ok(())
}

pub fn validate_color(color: &str) -> Result<()> {
    let ok = color.len() == 7 && color.starts_with('#') && color[1..].chars().all(|c| c.is_ascii_hexdigit());
    if ok {
        Ok(())
    } else {
        Err(Error::validation("color", "must be a hex color like #3b82f6"))
    }
}

/// Aliases are used in the quick switch (`#ALIAS`) and in exports, so they
/// are limited to characters that survive both.
pub fn validate_alias(alias: &str) -> Result<()> {
    let mut chars = alias.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphanumeric());
    let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !first_ok || !rest_ok || alias.len() > MAX_ALIAS_LEN {
        return Err(Error::validation(
            "alias",
            format!("1–{MAX_ALIAS_LEN} characters: letters, digits, '.', '_' or '-', starting with a letter or digit"),
        ));
    }
    Ok(())
}

pub fn validate_tz(tz: &str) -> Result<()> {
    tz.parse::<chrono_tz::Tz>()
        .map(|_| ())
        .map_err(|_| Error::validation("tz", format!("unknown time zone '{tz}'")))
}

impl ProjectInput {
    /// Validates and normalizes (trims, empty → `None`).
    pub fn normalized(mut self) -> Result<Self> {
        self.name = self.name.trim().to_string();
        validate_name("name", &self.name)?;
        self.alias = trimmed_opt(&self.alias);
        if let Some(alias) = &self.alias {
            validate_alias(alias)?;
        }
        validate_color(&self.color)?;
        self.client = trimmed_opt(&self.client);
        if self.budget_minutes.is_some_and(|m| m < 0) {
            return Err(Error::validation("budgetMinutes", "must not be negative"));
        }
        if !(1..=100).contains(&self.budget_warn_pct) {
            return Err(Error::validation("budgetWarnPct", "must be between 1 and 100"));
        }
        Ok(self)
    }
}

impl TagInput {
    pub fn normalized(mut self) -> Result<Self> {
        self.name = self.name.trim().to_string();
        validate_name("name", &self.name)?;
        validate_color(&self.color)?;
        self.group_name = trimmed_opt(&self.group_name);
        Ok(self)
    }
}

pub const MAX_CODE_LEN: usize = 32;

impl CostCenterInput {
    pub fn normalized(mut self) -> Result<Self> {
        self.code = self.code.trim().to_string();
        if self.code.is_empty() || self.code.chars().count() > MAX_CODE_LEN {
            return Err(Error::validation("code", format!("1–{MAX_CODE_LEN} characters")));
        }
        self.name = self.name.trim().to_string();
        validate_name("name", &self.name)?;
        if let (Some(from), Some(to)) = (self.valid_from, self.valid_to) {
            if to < from {
                return Err(Error::validation("validTo", "must not be before the start"));
            }
        }
        Ok(self)
    }
}

impl CostCenterAssignmentInput {
    pub fn validate(&self) -> Result<()> {
        if self.valid_to.is_some_and(|to| to < self.valid_from) {
            return Err(Error::validation("validTo", "must not be before the start"));
        }
        Ok(())
    }
}

pub fn validate_note(note: &str) -> Result<()> {
    if note.chars().count() > MAX_NOTE_LEN {
        return Err(Error::validation(
            "note",
            format!("must be at most {MAX_NOTE_LEN} characters"),
        ));
    }
    Ok(())
}

impl EntryInput {
    pub fn normalized(mut self) -> Result<Self> {
        if let Some(end) = self.end_at {
            if end < self.start_at {
                return Err(Error::validation("endAt", "must not be before the start"));
            }
        }
        if let Some(tz) = &self.tz {
            validate_tz(tz)?;
        }
        self.note = self.note.trim().to_string();
        validate_note(&self.note)?;
        self.tag_ids.sort();
        self.tag_ids.dedup();
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> ProjectInput {
        ProjectInput {
            name: "  Website  ".into(),
            alias: Some(" WEB-1 ".into()),
            color: "#3b82f6".into(),
            parent_id: None,
            client: Some("".into()),
            budget_minutes: Some(600),
            budget_period: BudgetPeriod::Total,
            budget_warn_pct: 80,
            billable_default: true,
            status: ProjectStatus::Active,
            cost_center_id: None,
        }
    }

    #[test]
    fn project_input_is_normalized() {
        let p = project().normalized().unwrap();
        assert_eq!(p.name, "Website");
        assert_eq!(p.alias.as_deref(), Some("WEB-1"));
        assert_eq!(p.client, None);
    }

    #[test]
    fn project_input_rejects_bad_values() {
        let bad_alias = ProjectInput {
            alias: Some("#web".into()),
            ..project()
        };
        assert!(bad_alias.normalized().is_err());
        let bad_color = ProjectInput {
            color: "blue".into(),
            ..project()
        };
        assert!(bad_color.normalized().is_err());
        let bad_name = ProjectInput {
            name: "   ".into(),
            ..project()
        };
        assert!(bad_name.normalized().is_err());
        let bad_warn = ProjectInput {
            budget_warn_pct: 0,
            ..project()
        };
        assert!(bad_warn.normalized().is_err());
    }

    #[test]
    fn entry_input_checks_range_and_tz() {
        let e = EntryInput {
            project_id: None,
            start_at: 100,
            end_at: Some(50),
            tz: None,
            note: String::new(),
            billable: None,
            tag_ids: vec![],
            cost_center_id: None,
        };
        assert!(e.clone().normalized().is_err());
        let bad_tz = EntryInput {
            end_at: Some(200),
            tz: Some("Mars/Base".into()),
            ..e.clone()
        };
        assert!(bad_tz.normalized().is_err());
        let ok = EntryInput {
            end_at: Some(200),
            tz: Some("Europe/Vienna".into()),
            tag_ids: vec!["b".into(), "a".into(), "b".into()],
            ..e
        };
        assert_eq!(ok.normalized().unwrap().tag_ids, vec!["a", "b"]);
    }

    #[test]
    fn enums_roundtrip_storage_values() {
        assert_eq!("auto_lunch".parse::<PauseKind>().unwrap(), PauseKind::AutoLunch);
        assert_eq!(PauseKind::AutoLunch.as_str(), "auto_lunch");
        assert!("nope".parse::<EntrySource>().is_err());
    }
}
