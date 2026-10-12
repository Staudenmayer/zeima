//! Addon manifest (`addon.toml`): identity, permissions, hooks, export
//! formats and the settings form. Pure parsing and validation; the app
//! loads and runs the WASM module (docs/addons.md).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// What an addon may do. The user grants all requested permissions when
/// installing; host functions check them on every call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Permission {
    /// Read time entries.
    #[serde(rename = "entries:read")]
    EntriesRead,
    /// Create time entries.
    #[serde(rename = "entries:write")]
    EntriesWrite,
    #[serde(rename = "projects:read")]
    ProjectsRead,
    #[serde(rename = "tags:read")]
    TagsRead,
    #[serde(rename = "cost-centers:read")]
    CostCentersRead,
    /// Show desktop notifications.
    #[serde(rename = "notify")]
    Notify,
    /// HTTP requests, only to `allowed_hosts`.
    #[serde(rename = "network")]
    Network,
}

/// Events and calls an addon reacts to. Each hook maps to an exported
/// WASM function (docs/addons.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Hook {
    #[serde(rename = "entry.started")]
    EntryStarted,
    #[serde(rename = "entry.stopped")]
    EntryStopped,
    #[serde(rename = "entry.created")]
    EntryCreated,
    #[serde(rename = "entry.updated")]
    EntryUpdated,
    #[serde(rename = "entry.deleted")]
    EntryDeleted,
    #[serde(rename = "day.closed")]
    DayClosed,
    /// Custom report export formats (`export_formats`).
    #[serde(rename = "report.export")]
    ReportExport,
    /// Extra options in the quick switch.
    #[serde(rename = "quickswitch.suggest")]
    QuickswitchSuggest,
    /// Called every `schedule_minutes`.
    #[serde(rename = "schedule.tick")]
    ScheduleTick,
}

impl Hook {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EntryStarted => "entry.started",
            Self::EntryStopped => "entry.stopped",
            Self::EntryCreated => "entry.created",
            Self::EntryUpdated => "entry.updated",
            Self::EntryDeleted => "entry.deleted",
            Self::DayClosed => "day.closed",
            Self::ReportExport => "report.export",
            Self::QuickswitchSuggest => "quickswitch.suggest",
            Self::ScheduleTick => "schedule.tick",
        }
    }

    /// Hooks fed by domain events (`crate::events::DomainEvent`).
    pub fn is_event(self) -> bool {
        !matches!(
            self,
            Self::ReportExport | Self::QuickswitchSuggest | Self::ScheduleTick
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ExportFormatDef {
    /// Unique within the addon.
    pub id: String,
    /// Shown in the report export menu.
    pub name: String,
    /// File extension without dot, e.g. `csv`.
    pub extension: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum SettingType {
    Text,
    Number,
    Boolean,
    /// One of `options`.
    Select,
    /// Text shown masked in the form.
    Password,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SettingOption {
    pub value: String,
    pub label: String,
}

/// One field of the addon's settings form. Values reach the addon as
/// Extism config strings under `key` (booleans as `true`/`false`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SettingField {
    /// Letters, digits and `_`.
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: SettingType,
    #[serde(default)]
    pub description: Option<String>,
    /// Default value as text.
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub options: Vec<SettingOption>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AddonManifest {
    /// Reverse-DNS id, e.g. `com.example.csv-export`.
    pub id: String,
    pub name: String,
    /// `MAJOR.MINOR.PATCH`.
    pub version: String,
    pub author: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub homepage: Option<String>,
    /// Oldest app version the addon works with.
    #[serde(default)]
    pub min_app_version: Option<String>,
    /// WASM module, relative to `addon.toml`.
    #[serde(default = "default_wasm")]
    pub wasm: String,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    /// Hosts reachable with the `network` permission, e.g. `api.github.com`
    /// or `*.example.com`.
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    #[serde(default)]
    pub hooks: Vec<Hook>,
    /// Interval for `schedule.tick`, at least 1.
    #[serde(default)]
    pub schedule_minutes: Option<u32>,
    /// Formats offered in the report export menu (`report.export`).
    #[serde(default)]
    pub export_formats: Vec<ExportFormatDef>,
    /// Settings form, in display order.
    #[serde(default)]
    pub settings: Vec<SettingField>,
}

fn default_wasm() -> String {
    "plugin.wasm".into()
}

/// `MAJOR.MINOR.PATCH` (pre-release suffixes are ignored).
pub fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.split(['-', '+']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

impl AddonManifest {
    pub fn parse(toml_src: &str) -> Result<Self> {
        let manifest: Self =
            toml::from_str(toml_src).map_err(|e| Error::validation("addon.toml", e.message().to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<()> {
        let id_ok = self.id.len() <= 100
            && self.id.contains('.')
            && self.id.split('.').all(|part| {
                !part.is_empty()
                    && part.starts_with(|c: char| c.is_ascii_lowercase())
                    && part
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            });
        if !id_ok {
            return Err(Error::validation(
                "id",
                "reverse-DNS id with lowercase letters, digits and '-', e.g. com.example.my-addon",
            ));
        }
        if self.name.trim().is_empty() {
            return Err(Error::validation("name", "must not be empty"));
        }
        if parse_version(&self.version).is_none() {
            return Err(Error::validation("version", "must be MAJOR.MINOR.PATCH"));
        }
        if let Some(min) = &self.min_app_version {
            if parse_version(min).is_none() {
                return Err(Error::validation("min_app_version", "must be MAJOR.MINOR.PATCH"));
            }
        }
        let wasm_ok = self.wasm.ends_with(".wasm")
            && !self.wasm.starts_with('/')
            && !self.wasm.contains('\\')
            && !self.wasm.split('/').any(|p| p == ".." || p.is_empty());
        if !wasm_ok {
            return Err(Error::validation("wasm", "a .wasm file inside the addon folder"));
        }
        let network = self.permissions.contains(&Permission::Network);
        if network && self.allowed_hosts.is_empty() {
            return Err(Error::validation(
                "allowed_hosts",
                "the network permission needs at least one host",
            ));
        }
        if !network && !self.allowed_hosts.is_empty() {
            return Err(Error::validation(
                "allowed_hosts",
                "only allowed with the network permission",
            ));
        }
        for host in &self.allowed_hosts {
            let bare = host.strip_prefix("*.").unwrap_or(host);
            let ok = bare.contains('.')
                && bare
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
            if !ok {
                return Err(Error::validation(
                    "allowed_hosts",
                    format!("'{host}' is not a host name (no scheme, path or port)"),
                ));
            }
        }
        let exports = self.hooks.contains(&Hook::ReportExport);
        if exports == self.export_formats.is_empty() {
            return Err(Error::validation(
                "export_formats",
                "the report.export hook and export_formats go together",
            ));
        }
        let mut format_ids = std::collections::HashSet::new();
        for f in &self.export_formats {
            if f.id.is_empty() || !format_ids.insert(&f.id) {
                return Err(Error::validation(
                    "export_formats",
                    format!("duplicate or empty id '{}'", f.id),
                ));
            }
            if f.extension.is_empty() || !f.extension.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Err(Error::validation(
                    "export_formats",
                    format!("bad extension '{}'", f.extension),
                ));
            }
        }
        let ticks = self.hooks.contains(&Hook::ScheduleTick);
        match (ticks, self.schedule_minutes) {
            (true, Some(m)) if m >= 1 => {}
            (true, _) => {
                return Err(Error::validation(
                    "schedule_minutes",
                    "schedule.tick needs schedule_minutes ≥ 1",
                ))
            }
            (false, Some(_)) => {
                return Err(Error::validation(
                    "schedule_minutes",
                    "only allowed with the schedule.tick hook",
                ))
            }
            (false, None) => {}
        }
        let mut keys = std::collections::HashSet::new();
        for field in &self.settings {
            let key_ok = !field.key.is_empty()
                && field.key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && keys.insert(&field.key);
            if !key_ok {
                return Err(Error::validation(
                    "settings",
                    format!("bad or duplicate key '{}'", field.key),
                ));
            }
            if (field.kind == SettingType::Select) == field.options.is_empty() {
                return Err(Error::validation(
                    "settings",
                    format!("'{}': options are required for select fields only", field.key),
                ));
            }
        }
        Ok(())
    }

    /// Whether this app version is new enough.
    pub fn supports_app(&self, app_version: &str) -> bool {
        match (&self.min_app_version, parse_version(app_version)) {
            (Some(min), Some(app)) => parse_version(min).is_some_and(|min| app >= min),
            _ => true,
        }
    }
}

/// JSON Schema of `addon.toml` (`crates/time-core/addon.schema.json`).
pub fn json_schema() -> String {
    let schema = schemars::schema_for!(AddonManifest);
    serde_json::to_string_pretty(&schema).expect("schema serializes") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"
id = "com.example.csv-export"
name = "CSV export"
version = "1.2.0"
author = "Example"
min_app_version = "0.1.0"
permissions = ["entries:read", "network"]
allowed_hosts = ["api.example.com", "*.example.org"]
hooks = ["report.export", "schedule.tick", "entry.started"]
schedule_minutes = 15

[[export_formats]]
id = "csv"
name = "CSV (custom)"
extension = "csv"

[[settings]]
key = "delimiter"
label = "Delimiter"
type = "select"
default = ";"
options = [{ value = ";", label = "Semicolon" }, { value = ",", label = "Comma" }]
"#;

    #[test]
    fn parses_a_full_manifest() {
        let m = AddonManifest::parse(EXAMPLE).unwrap();
        assert_eq!(m.wasm, "plugin.wasm");
        assert_eq!(m.hooks[0], Hook::ReportExport);
        assert_eq!(m.settings[0].kind, SettingType::Select);
        assert!(m.supports_app("0.1.0"));
        assert!(!m.supports_app("0.0.9"));
    }

    #[test]
    fn rejects_inconsistent_manifests() {
        let bad = |from: &str, to: &str| {
            let src = EXAMPLE.replace(from, to);
            AddonManifest::parse(&src).unwrap_err()
        };
        bad("com.example.csv-export", "CSV Export");
        bad("version = \"1.2.0\"", "version = \"1.2\"");
        bad("\"entries:read\", \"network\"", "\"entries:read\"");
        bad("\"api.example.com\"", "\"https://api.example.com\"");
        bad("schedule_minutes = 15", "");
        bad("\"report.export\", ", "");
        bad("type = \"select\"", "type = \"text\"");
        bad(
            "id = \"com.example.csv-export\"",
            "id = \"com.example.csv-export\"\nwasm = \"../evil.wasm\"",
        );
        bad("author = \"Example\"", "author = \"Example\"\nunknown = 1");
    }

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(parse_version("1.10.0-beta"), Some((1, 10, 0)));
        assert!(parse_version("1.10.0") > parse_version("1.9.9"));
        assert_eq!(parse_version("1.2"), None);
    }

    /// Keeps `addon.schema.json` in sync with the Rust types; CI fails if
    /// the committed file differs.
    #[test]
    fn json_schema_is_up_to_date() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/addon.schema.json");
        std::fs::write(path, json_schema()).unwrap();
    }
}
