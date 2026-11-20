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
    /// Read the activities (what kind of work an entry is).
    #[serde(rename = "activities:read")]
    ActivitiesRead,
    #[serde(rename = "tags:read")]
    TagsRead,
    #[serde(rename = "cost-centers:read")]
    CostCentersRead,
    /// Read the person's meetings and what they answered (organization workspaces).
    #[serde(rename = "meetings:read")]
    MeetingsRead,
    /// Read the person's absences and their approval status.
    #[serde(rename = "absences:read")]
    AbsencesRead,
    /// Read work days: target, worked time, balance, holidays.
    #[serde(rename = "work:read")]
    WorkRead,
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
    /// A meeting of the person was created, changed, cancelled or answered.
    #[serde(rename = "meeting.changed")]
    MeetingChanged,
    /// An absence of the person was requested, changed, decided or deleted.
    #[serde(rename = "absence.changed")]
    AbsenceChanged,
    /// Custom report export formats (`export_formats`).
    #[serde(rename = "report.export")]
    ReportExport,
    /// Extra options in the quick switch.
    #[serde(rename = "quickswitch.suggest")]
    QuickswitchSuggest,
    /// Called every `schedule_minutes`.
    #[serde(rename = "schedule.tick")]
    ScheduleTick,
    /// Items shown in the timesheet calendar, e.g. calendar events.
    #[serde(rename = "timeline.events")]
    TimelineEvents,
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
            Self::MeetingChanged => "meeting.changed",
            Self::AbsenceChanged => "absence.changed",
            Self::ReportExport => "report.export",
            Self::QuickswitchSuggest => "quickswitch.suggest",
            Self::ScheduleTick => "schedule.tick",
            Self::TimelineEvents => "timeline.events",
        }
    }

    /// Hooks fed by domain events (`crate::events::DomainEvent`).
    pub fn is_event(self) -> bool {
        !matches!(
            self,
            Self::ReportExport | Self::QuickswitchSuggest | Self::ScheduleTick | Self::TimelineEvents
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

/// An account the user can connect on the Addons page (OAuth 2
/// authorization code flow with PKCE in the system browser). The app keeps
/// the refresh token in the OS keychain; the addon only gets access tokens
/// (`host::oauth_token`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct OAuthProvider {
    /// Unique within the addon; letters, digits and `_`.
    pub id: String,
    /// Shown to users, e.g. `Microsoft 365`.
    pub name: String,
    /// `https://` authorization endpoint.
    pub auth_url: String,
    /// `https://` token endpoint.
    pub token_url: String,
    pub client_id: String,
    /// Only for providers that require one for desktop apps (Google);
    /// it is not secret in an app that users install.
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    /// Extra query parameters for the authorization URL, e.g.
    /// `{ access_type = "offline", prompt = "consent" }` for Google.
    #[serde(default)]
    pub auth_params: std::collections::BTreeMap<String, String>,
    /// Host of the loopback redirect, `127.0.0.1` (default) or `localhost`,
    /// as registered with the provider. The redirect URI is
    /// `http://<host>:<port>` with a free port.
    #[serde(default)]
    pub redirect_host: Option<String>,
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
    /// Keys of text settings that hold an address the addon may request
    /// (an ICS feed, a self-hosted server). The host of each value is added
    /// to the allowed hosts, so the user decides where the addon connects.
    #[serde(default)]
    pub host_settings: Vec<String>,
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
    /// Accounts the user can connect (needs `network`).
    #[serde(default)]
    pub oauth: Vec<OAuthProvider>,
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

/// Whether a lower-case host name is numeric, local or otherwise not a public name.
pub fn is_internal_host(lower: &str) -> bool {
    lower.trim_matches(['[', ']']).parse::<std::net::IpAddr>().is_ok()
        || lower.split('.').all(|p| p.chars().all(|c| c.is_ascii_digit()))
        || lower == "localhost"
        || lower.ends_with(".localhost")
        || lower.ends_with(".local")
        || lower.ends_with(".internal")
        || lower.ends_with(".lan")
}

/// Host name of an address a user typed into a setting: `https://`,
/// `http://` and `webcal://` are accepted, a port or path is ignored.
/// `None` if there is no usable host.
pub fn host_of_address(address: &str) -> Option<String> {
    let rest = address.trim();
    let rest = rest.split_once("://").map_or(rest, |(_, r)| r);
    // The authority ends at the first slash, backslash, '?' or '#' (a backslash counts as a slash, as in the url crate).
    let authority = rest.split(['/', '\\', '?', '#']).next()?;
    let rest = authority.rsplit_once('@').map_or(authority, |(_, r)| r);
    let host = rest.split(':').next()?.to_ascii_lowercase();
    let ok = host.contains('.')
        && !host.starts_with('.')
        && !host.ends_with('.')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
    ok.then_some(host)
}

/// Suffixes under which strangers have their own names; a wildcard over one of them is refused.
const PUBLIC_SUFFIXES: &[&str] = &[
    "co.uk",
    "org.uk",
    "ac.uk",
    "gov.uk",
    "me.uk",
    "com.au",
    "net.au",
    "org.au",
    "co.nz",
    "co.jp",
    "co.kr",
    "co.in",
    "co.za",
    "com.br",
    "com.cn",
    "com.mx",
    "com.tr",
    "com.sg",
    "com.hk",
    "co.il",
    "com.ar",
    "github.io",
    "gitlab.io",
    "vercel.app",
    "netlify.app",
    "pages.dev",
    "workers.dev",
    "herokuapp.com",
    "azurewebsites.net",
    "cloudfront.net",
    "amazonaws.com",
    "appspot.com",
    "web.app",
    "firebaseapp.com",
    "onrender.com",
];

/// Whether `id` is a valid add-on id (reverse-DNS, lowercase letters, digits and '-'). Anything that takes an id
/// from outside and uses it as a folder name must check this first.
pub fn is_addon_id(id: &str) -> bool {
    id.len() <= 100
        && id.contains('.')
        && id.split('.').all(|part| {
            !part.is_empty()
                && part.starts_with(|c: char| c.is_ascii_lowercase())
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

/// Characters that must not appear in text the app shows as its own: control characters, line and paragraph
/// separators, bidirectional overrides/isolates and invisible format characters (soft hyphen, zero-width, BOM).
pub fn is_unsafe_display_char(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{AD}'
                | '\u{61C}'
                | '\u{200B}'..='\u{200F}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{2069}'
                | '\u{FEFF}'
        )
}

/// `text` without the characters of [`is_unsafe_display_char`].
pub fn display_safe(text: &str) -> String {
    text.chars().filter(|c| !is_unsafe_display_char(*c)).collect()
}

fn check_display(field: &'static str, text: &str, max: usize) -> Result<()> {
    if text.chars().count() > max {
        return Err(Error::validation(
            field,
            format!("must be at most {max} characters"),
        ));
    }
    if text.chars().any(is_unsafe_display_char) {
        return Err(Error::validation(
            field,
            "must not contain control or direction-changing characters",
        ));
    }
    Ok(())
}

impl AddonManifest {
    /// Allowed hosts: the manifest's list plus the hosts of the user's
    /// `host_settings` values (`values` maps setting keys to their value).
    pub fn effective_hosts(&self, values: &std::collections::BTreeMap<String, String>) -> Vec<String> {
        let mut hosts = self.allowed_hosts.clone();
        for key in &self.host_settings {
            if let Some(host) = values.get(key).and_then(|v| host_of_address(v)) {
                if !hosts.contains(&host) {
                    hosts.push(host);
                }
            }
        }
        hosts
    }

    pub fn parse(toml_src: &str) -> Result<Self> {
        let manifest: Self =
            toml::from_str(toml_src).map_err(|e| Error::validation("addon.toml", e.message().to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<()> {
        if !is_addon_id(&self.id) {
            return Err(Error::validation(
                "id",
                "reverse-DNS id with lowercase letters, digits and '-', e.g. com.example.my-addon",
            ));
        }
        if self.name.trim().is_empty() {
            return Err(Error::validation("name", "must not be empty"));
        }
        check_display("name", &self.name, 80)?;
        check_display("author", &self.author, 80)?;
        for p in &self.oauth {
            check_display("oauth.name", &p.name, 60)?;
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
            // Drive letters (`C:/x.wasm`) and NTFS streams (`a.txt:x.wasm`) are paths of their own on Windows.
            && !self.wasm.contains(':')
            && !self.wasm.chars().any(|c| c.is_control())
            && !self.wasm.split('/').any(|p| p == ".." || p == "." || p.is_empty());
        if !wasm_ok {
            return Err(Error::validation("wasm", "a .wasm file inside the addon folder"));
        }
        let network = self.permissions.contains(&Permission::Network);
        if network && self.allowed_hosts.is_empty() && self.host_settings.is_empty() {
            return Err(Error::validation(
                "allowed_hosts",
                "the network permission needs at least one host or host_settings",
            ));
        }
        if !network && (!self.allowed_hosts.is_empty() || !self.host_settings.is_empty()) {
            return Err(Error::validation(
                "allowed_hosts",
                "only allowed with the network permission",
            ));
        }
        for key in &self.host_settings {
            let is_text = self
                .settings
                .iter()
                .any(|f| &f.key == key && matches!(f.kind, SettingType::Text | SettingType::Password));
            if !is_text {
                return Err(Error::validation(
                    "host_settings",
                    format!("'{key}' must be the key of a text setting"),
                ));
            }
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
            // An add-on connects to the internet, not to this machine or the network around it.
            let lower = bare.to_ascii_lowercase();
            if is_internal_host(&lower) {
                return Err(Error::validation(
                    "allowed_hosts",
                    format!(
                        "'{host}' is a local or numeric address; add-ons may only name public host names"
                    ),
                ));
            }
            // A wildcard over a whole public suffix (`*.co.uk`, `*.github.io`) would be every site below it.
            if host.starts_with("*.") && PUBLIC_SUFFIXES.contains(&lower.as_str()) {
                return Err(Error::validation(
                    "allowed_hosts",
                    format!("'{host}' covers every site under a public suffix; name the host"),
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
        let mut provider_ids = std::collections::HashSet::new();
        for p in &self.oauth {
            let id_ok = !p.id.is_empty()
                && p.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && provider_ids.insert(&p.id);
            if !id_ok {
                return Err(Error::validation(
                    "oauth",
                    format!("bad or duplicate id '{}'", p.id),
                ));
            }
            if !network {
                return Err(Error::validation("oauth", "needs the network permission"));
            }
            if p.name.trim().is_empty() || p.client_id.trim().is_empty() {
                return Err(Error::validation(
                    "oauth",
                    format!("'{}': name and client_id must not be empty", p.id),
                ));
            }
            for url in [&p.auth_url, &p.token_url] {
                let host = url
                    .strip_prefix("https://")
                    .and_then(|rest| rest.split(['/', '?']).next());
                if host.is_none_or(|h| h.is_empty() || h.contains('@')) {
                    return Err(Error::validation(
                        "oauth",
                        format!("'{}': '{url}' is not an https:// URL", p.id),
                    ));
                }
                // The app itself calls these addresses: they must be public host names, like allowed_hosts.
                let name = host
                    .and_then(|h| host_of_address(&format!("https://{h}")))
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if name.is_empty() || is_internal_host(&name) {
                    return Err(Error::validation(
                        "oauth",
                        format!(
                            "'{}': '{url}' must name a public host, not a local or numeric address",
                            p.id
                        ),
                    ));
                }
            }
            if let Some(host) = &p.redirect_host {
                if host != "127.0.0.1" && host != "localhost" {
                    return Err(Error::validation(
                        "oauth",
                        format!("'{}': redirect_host must be 127.0.0.1 or localhost", p.id),
                    ));
                }
            }
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

/// JSON Schema of `addon.toml` (`crates/zeima-core/addon.schema.json`).
pub fn json_schema() -> String {
    let schema = schemars::schema_for!(AddonManifest);
    serde_json::to_string_pretty(&schema).expect("schema serializes") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invisible_format_characters_are_not_displayable() {
        for c in [
            '\u{AD}', '\u{61C}', '\u{200B}', '\u{200C}', '\u{200D}', '\u{200E}', '\u{200F}', '\u{2060}',
            '\u{2062}', '\u{2064}', '\u{FEFF}', '\u{202E}', '\u{2066}', '\u{7}', '\n',
        ] {
            assert!(is_unsafe_display_char(c), "{:x}", c as u32);
        }
        assert!(!is_unsafe_display_char('a') && !is_unsafe_display_char('ä') && !is_unsafe_display_char(' '));
        assert_eq!(display_safe("a\u{200B}b\u{FEFF}c\u{AD}"), "abc");
    }

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

[[oauth]]
id = "google"
name = "Google"
auth_url = "https://accounts.google.com/o/oauth2/v2/auth"
token_url = "https://oauth2.googleapis.com/token"
client_id = "123.apps.googleusercontent.com"
scopes = ["https://www.googleapis.com/auth/calendar.readonly"]
auth_params = { access_type = "offline", prompt = "consent" }
"#;

    #[test]
    fn parses_a_full_manifest() {
        let m = AddonManifest::parse(EXAMPLE).unwrap();
        assert_eq!(m.wasm, "plugin.wasm");
        assert_eq!(m.hooks[0], Hook::ReportExport);
        assert_eq!(m.settings[0].kind, SettingType::Select);
        assert_eq!(m.oauth[0].auth_params["access_type"], "offline");
        assert!(m.supports_app("0.1.0"));
        assert!(!m.supports_app("0.0.9"));
    }

    #[test]
    fn oauth_endpoints_must_be_public_https_hosts() {
        let base = |url: &str| {
            format!(
                "id = \"com.example.poc\"\nname = \"p\"\nversion = \"1.0.0\"\nauthor = \"x\"\nwasm = \"a.wasm\"\npermissions = [\"network\"]\nallowed_hosts = [\"api.example.com\"]\n[[oauth]]\nid = \"p\"\nname = \"P\"\nclient_id = \"c\"\nauth_url = \"https://accounts.example.com/auth\"\ntoken_url = \"{url}\"\n"
            )
        };
        for bad in [
            "https://127.0.0.1:8443/t",
            "https://169.254.169.254/x",
            "https://10.0.0.5/x",
            "https://[::1]/x",
            "https://intranet.corp.local/t",
            "https://localhost/t",
        ] {
            assert!(AddonManifest::parse(&base(bad)).is_err(), "{bad}");
        }
        assert!(AddonManifest::parse(&base("https://oauth2.example.net/token")).is_ok());
    }

    #[test]
    fn host_of_address_reads_the_host() {
        assert_eq!(
            host_of_address("https://Cal.Example.com/a/b.ics?x=1").as_deref(),
            Some("cal.example.com")
        );
        assert_eq!(
            host_of_address("webcal://example.org/x.ics").as_deref(),
            Some("example.org")
        );
        assert_eq!(
            host_of_address("http://user:pw@example.org:8080/x").as_deref(),
            Some("example.org")
        );
        // A backslash ends the authority like a slash, so what follows it is not the host.
        assert_eq!(
            host_of_address("http://example.org\\@127.0.0.1/x").as_deref(),
            Some("example.org")
        );
        assert_eq!(host_of_address("localhost"), None);
        assert_eq!(host_of_address(""), None);
        assert_eq!(host_of_address("https://exa mple.com"), None);
    }

    #[test]
    fn hosts_can_come_from_settings() {
        let src = EXAMPLE.replace(
            "allowed_hosts = [\"api.example.com\", \"*.example.org\"]\n",
            "host_settings = [\"feed\"]\n",
        ) + "\n[[settings]]\nkey = \"feed\"\nlabel = \"Feed\"\ntype = \"text\"\n";
        let m = AddonManifest::parse(&src).unwrap();
        let mut values = std::collections::BTreeMap::new();
        assert!(m.effective_hosts(&values).is_empty());
        values.insert("feed".into(), "webcal://cal.example.com/me.ics".into());
        assert_eq!(m.effective_hosts(&values), vec!["cal.example.com".to_string()]);
        // Must point at a text setting.
        let bad = src.replace("host_settings = [\"feed\"]", "host_settings = [\"delimiter\"]");
        assert!(AddonManifest::parse(&bad).is_err());
    }

    #[test]
    fn only_reverse_dns_ids_are_add_on_ids() {
        for ok in ["com.example.csv-export", "a.b", "zeima.meetings"] {
            assert!(is_addon_id(ok), "{ok}");
        }
        for bad in [
            "C:", ".", "a/b", "..", "x:y", "", "a", "a..b", "a.b\\c", "C:.a", "A.b", "a.b.", "a.1b",
        ] {
            assert!(!is_addon_id(bad), "{bad}");
        }
    }

    #[test]
    fn display_names_are_short_and_single_line() {
        let with = |from: &str, to: &str| AddonManifest::parse(&EXAMPLE.replace(from, to));
        assert!(with("name = \"CSV export\"", "name = \"CSV\\nSigned by X\"").is_err());
        assert!(with("author = \"Example\"", "author = \"Ex\\u2028ample\"").is_err());
        assert!(with("author = \"Example\"", "author = \"Ex\\u202Eample\"").is_err());
        assert!(with("author = \"Example\"", "author = \"Ex\\u2066ample\"").is_err());
        assert!(with("name = \"Google\"", "name = \"Goo\\rgle\"").is_err());
        let long = "x".repeat(81);
        assert!(with("name = \"CSV export\"", &format!("name = \"{long}\"")).is_err());
        assert!(with("author = \"Example\"", &format!("author = \"{long}\"")).is_err());
        assert!(with("name = \"Google\"", &format!("name = \"{}\"", "x".repeat(61))).is_err());
        assert!(with("name = \"CSV export\"", &format!("name = \"{}\"", "x".repeat(80))).is_ok());
        assert!(with("name = \"Google\"", &format!("name = \"{}\"", "x".repeat(60))).is_ok());
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
        // Local and numeric hosts, and wildcards over a whole public suffix.
        bad("\"api.example.com\"", "\"127.0.0.1\"");
        bad("\"api.example.com\"", "\"printer.local\"");
        bad("\"api.example.com\"", "\"169.254.169.254\"");
        bad("\"*.example.org\"", "\"*.co.uk\"");
        bad("\"*.example.org\"", "\"*.github.io\"");
        bad("schedule_minutes = 15", "");
        bad("\"report.export\", ", "");
        bad("type = \"select\"", "type = \"text\"");
        bad(
            "id = \"com.example.csv-export\"",
            "id = \"com.example.csv-export\"\nwasm = \"../evil.wasm\"",
        );
        for evil in ["C:/Users/x/y.wasm", "a.txt:x.wasm", "./x.wasm", "a/./x.wasm"] {
            bad(
                "id = \"com.example.csv-export\"",
                &format!("id = \"com.example.csv-export\"\nwasm = \"{evil}\""),
            );
        }
        bad("author = \"Example\"", "author = \"Example\"\nunknown = 1");
        bad(
            "https://oauth2.googleapis.com/token",
            "http://oauth2.googleapis.com/token",
        );
        bad("id = \"google\"", "id = \"google-cal\"");
        bad(
            "client_id = \"123",
            "redirect_host = \"example.com\"\nclient_id = \"123",
        );
        // Accounts are useless without network access.
        bad(
            "\"entries:read\", \"network\"]\nallowed_hosts = [\"api.example.com\", \"*.example.org\"]",
            "\"entries:read\"]",
        );
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
