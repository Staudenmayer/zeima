//! Addon trust, the addon registry and the organizations' addon rules (docs/addons.md, "Signing and
//! the registry").
//!
//! * **Trust**: a package is signed by a publisher; the app trusts the publishers its own build lists
//!   ([`BUILTIN`]) and the ones the signed-in server's registry names. The result of the check is kept
//!   next to the installed addon (`signature.json`).
//! * **Registry**: the signed-in server lists addons (`GET /v1/addons/registry`); installing one
//!   downloads the package, checks its hash and signature, and then goes through the same steps as a
//!   file the person picked.
//! * **Rules**: every organization the person belongs to may allow only some addons and may require a
//!   signature (`GET /v1/orgs/:id/addons`). The strictest rule wins; an addon that breaks one is
//!   neither installed nor run. The rules are kept on disk so they also apply offline.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;
use tauri::{AppHandle, Manager, State};
use zeima_core::addon_sig::{self, SignatureCheck, SignatureStatus, TrustedKey};

use crate::account::{self, discover};
use crate::addons::{addons_dir, Addons};
use crate::commands::{CmdError, CmdResult};
use crate::commands_addons::{install_package, read_package_bytes, AddonInfo};
use crate::state::AppState;

/// Publisher keys built into this app, the vendor's: `key-id=Name=public-key` separated by `;`. A release build sets
/// them with the environment variable `ZEIMA_ADDON_VENDOR_KEYS` at compile time, so the key is part of the signed
/// app and cannot be replaced by a server. Installations add their own publishers through their server's registry;
/// those are shown as "server trusted", never as the vendor's.
pub const BUILTIN: &str = match option_env!("ZEIMA_ADDON_VENDOR_KEYS") {
    Some(keys) => keys,
    None => "",
};

/// The built-in keys as publishers.
pub fn builtin_publishers() -> Vec<Publisher> {
    parse_builtin(BUILTIN)
}

fn parse_builtin(spec: &str) -> Vec<Publisher> {
    spec.split(';')
        .filter_map(|entry| {
            let mut parts = entry.trim().splitn(3, '=');
            let (id, name, key) = (parts.next()?, parts.next()?, parts.next()?);
            (!id.is_empty() && !name.is_empty() && !key.is_empty()).then(|| Publisher {
                key_id: id.into(),
                name: name.into(),
                public_key: key.into(),
            })
        })
        .collect()
}

const SIGNATURE_RECORD: &str = "signature.json";
const STATE_FILE: &str = ".registry-state.json";
const MAX_PACKAGE_BYTES: u64 = 60 * 1024 * 1024;

fn err(code: &str, message: impl ToString) -> CmdError {
    CmdError::other(code, message)
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Publisher {
    pub key_id: String,
    pub name: String,
    pub public_key: String,
}

/// What one organization allows.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OrgRule {
    pub org_id: String,
    pub org_name: String,
    /// `open` or `allowlist`.
    pub mode: String,
    pub require_signed: bool,
    pub allowed: Vec<String>,
}

impl OrgRule {
    /// Why this organization does not let `addon_id` (with this signature) in; `None` if it does.
    pub fn refuses(&self, addon_id: &str, signature: &SignatureCheck) -> Option<String> {
        if self.mode == "allowlist" && !self.allowed.iter().any(|a| a.eq_ignore_ascii_case(addon_id)) {
            return Some(format!(
                "{} only allows the addons on its list; {addon_id} is not on it.",
                self.org_name
            ));
        }
        if self.require_signed && !signature.is_trusted() {
            return Some(format!(
                "{} only allows addons signed by a publisher it trusts.",
                self.org_name
            ));
        }
        None
    }
}

/// The first reason any organization gives; `None` if all of them allow it.
pub fn refused_by(rules: &[OrgRule], addon_id: &str, signature: &SignatureCheck) -> Option<String> {
    rules.iter().find_map(|r| r.refuses(addon_id, signature))
}

/// The keys this app trusts: its own and the publishers of the server's registry.
pub fn trusted_keys(publishers: &[Publisher]) -> Vec<TrustedKey> {
    keys_of(&builtin_publishers(), publishers)
}

/// `vendor` keys come first, so a server cannot override a vendor key id with its own key.
fn keys_of(vendor: &[Publisher], server: &[Publisher]) -> Vec<TrustedKey> {
    let make = |p: &Publisher, is_vendor: bool| {
        Some(TrustedKey {
            key_id: p.key_id.clone(),
            name: p.name.clone(),
            key: addon_sig::parse_public_key(&p.public_key)?,
            vendor: is_vendor,
        })
    };
    vendor
        .iter()
        .filter_map(|p| make(p, true))
        .chain(
            server
                .iter()
                .filter(|p| !vendor.iter().any(|v| v.key_id == p.key_id))
                .filter_map(|p| make(p, false)),
        )
        .collect()
}

/// What was found out about an installed addon's signature when it was installed.
pub fn installed_signature(dir: &Path) -> SignatureCheck {
    std::fs::read_to_string(dir.join(SIGNATURE_RECORD))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(SignatureCheck::unsigned)
}

pub fn write_signature(dir: &Path, check: &SignatureCheck) -> std::io::Result<()> {
    let text = serde_json::to_string(check).map_err(std::io::Error::other)?;
    std::fs::write(dir.join(SIGNATURE_RECORD), text)
}

// --------------------------------------------------------------------------
// Persisted state: publishers and rules, so the app can check offline.
// --------------------------------------------------------------------------

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    publishers: Vec<Publisher>,
    #[serde(default)]
    rules: Vec<OrgRule>,
}

fn state_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    addons_dir(app).ok().map(|d| d.join(STATE_FILE))
}

pub fn load_state(app: &AppHandle) {
    let Some(saved) = state_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<Saved>(&s).ok())
    else {
        return;
    };
    let addons = app.state::<Addons>();
    addons.set_trust(saved.publishers, saved.rules);
}

fn save_state(app: &AppHandle) {
    let addons = app.state::<Addons>();
    let (publishers, rules) = addons.trust_snapshot();
    if let (Some(path), Ok(text)) = (
        state_path(app),
        serde_json::to_string(&Saved { publishers, rules }),
    ) {
        let _ = std::fs::write(path, text);
    }
}

/// Forgets what a server told this app to trust (its publishers and its organizations' rules): after signing out or
/// connecting to another server, the old one no longer vouches for anything.
pub fn forget_server_trust(app: &AppHandle) {
    app.state::<Addons>().set_trust(Vec::new(), Vec::new());
    save_state(app);
}

// --------------------------------------------------------------------------
// The server
// --------------------------------------------------------------------------

/// The API address and an access token; `None` when signed out.
async fn session(app: &AppHandle) -> Result<Option<(String, String)>, CmdError> {
    let Some((server, token)) = account::current_token(app).await.map_err(|e| err("server", e))? else {
        return Ok(None);
    };
    let (discovery, _) = tauri::async_runtime::spawn_blocking(move || discover(&server))
        .await
        .map_err(|e| err("io", e))?
        .map_err(|e| err("server", e))?;
    Ok(Some((discovery.api_url.trim_end_matches('/').to_string(), token)))
}

async fn get_json<T: serde::de::DeserializeOwned + Send + 'static>(
    api: &str,
    path: &str,
    token: &str,
) -> Result<T, CmdError> {
    let (url, token) = (format!("{api}{path}"), token.to_string());
    let (status, text) =
        tauri::async_runtime::spawn_blocking(move || crate::org::call("GET", &url, &token, None))
            .await
            .map_err(|e| err("io", e))?
            .map_err(|e| err("server", format!("could not reach the server: {e}")))?;
    if !(200..300).contains(&status) {
        return Err(err("server", format!("the server answered {status}")));
    }
    serde_json::from_str(&text).map_err(|e| err("server", e))
}

fn download(url: &str, token: &str) -> Result<Vec<u8>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header("Authorization", &format!("Bearer {token}"))
        .call()
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("the server answered {}", response.status().as_u16()));
    }
    response
        .body_mut()
        .with_config()
        .limit(MAX_PACKAGE_BYTES)
        .read_to_vec()
        .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexEntry {
    id: String,
    name: String,
    version: String,
    description: String,
    author: String,
    min_app_version: Option<String>,
    permissions: Vec<String>,
    hooks: Vec<String>,
    file: String,
    sha256: String,
    size: u64,
    key_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Index {
    publishers: Vec<Publisher>,
    addons: Vec<IndexEntry>,
}

/// An addon the server offers.
#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RegistryAddon {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub permissions: Vec<String>,
    pub hooks: Vec<String>,
    pub size: u32,
    pub installed_version: Option<String>,
    /// The publisher's name when the package says it is signed by a publisher this app trusts.
    pub signer: Option<String>,
    pub compatible: bool,
    pub blocked: Option<String>,
}

#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RegistryView {
    pub addons: Vec<RegistryAddon>,
    /// Why the list could not be read (signed out, server unreachable).
    pub error: Option<String>,
}

async fn fetch_index(app: &AppHandle) -> Result<(String, String, Index), CmdError> {
    let Some((api, token)) = session(app).await? else {
        return Err(err("signed_out", "sign in to see the addons of your server"));
    };
    let index: Index = get_json(&api, "/v1/addons/registry", &token).await?;
    app.state::<Addons>().set_publishers(index.publishers.clone());
    save_state(app);
    Ok((api, token, index))
}

#[tauri::command]
#[specta::specta]
pub async fn addon_registry(
    app: AppHandle,
    state: State<'_, AppState>,
    addons: State<'_, Addons>,
) -> CmdResult<RegistryView> {
    let index = match fetch_index(&app).await {
        Ok((_, _, index)) => index,
        Err(e) => {
            return Ok(RegistryView {
                addons: Vec::new(),
                error: Some(e.message),
            })
        }
    };
    let version = app.package_info().version.to_string();
    let mut out = Vec::new();
    // The newest version of each addon.
    let mut newest: Vec<&IndexEntry> = Vec::new();
    for e in &index.addons {
        match newest.iter().position(|n| n.id == e.id) {
            Some(i)
                if zeima_core::addon::parse_version(&newest[i].version)
                    >= zeima_core::addon::parse_version(&e.version) => {}
            Some(i) => newest[i] = e,
            None => newest.push(e),
        }
    }
    for e in newest {
        let signer = e.key_id.as_ref().and_then(|k| {
            trusted_keys(&index.publishers)
                .into_iter()
                .find(|t| &t.key_id == k)
                .map(|t| t.name)
        });
        let vendor_key = e
            .key_id
            .as_ref()
            .is_some_and(|k| builtin_publishers().iter().any(|p| &p.key_id == k));
        let claimed = SignatureCheck {
            status: match (signer.is_some(), vendor_key) {
                (true, true) => SignatureStatus::Trusted,
                (true, false) => SignatureStatus::ServerTrusted,
                (false, _) if e.key_id.is_some() => SignatureStatus::UnknownKey,
                _ => SignatureStatus::Unsigned,
            },
            key_id: e.key_id.clone(),
            signer: signer.clone(),
            fingerprint: None,
        };
        out.push(RegistryAddon {
            id: e.id.clone(),
            name: e.name.clone(),
            version: e.version.clone(),
            description: e.description.clone(),
            author: e.author.clone(),
            permissions: e.permissions.clone(),
            hooks: e.hooks.clone(),
            size: e.size.min(u64::from(u32::MAX)) as u32,
            installed_version: state.core.get_addon(&e.id).await.ok().map(|r| r.version),
            signer,
            compatible: e.min_app_version.as_deref().is_none_or(|m| {
                zeima_core::addon::parse_version(&version) >= zeima_core::addon::parse_version(m)
            }),
            blocked: addons.install_blocked(&e.id, &claimed),
        });
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    Ok(RegistryView {
        addons: out,
        error: None,
    })
}

/// Downloads an addon from the server's registry and installs it, after checking its hash and its
/// signature and the rules of the person's organizations.
#[tauri::command]
#[specta::specta]
pub async fn addon_registry_install(
    app: AppHandle,
    state: State<'_, AppState>,
    addons: State<'_, Addons>,
    id: String,
    version: String,
) -> CmdResult<AddonInfo> {
    let (api, token, index) = fetch_index(&app).await?;
    let entry = index
        .addons
        .iter()
        .find(|e| e.id == id && e.version == version)
        .ok_or_else(|| err("not_found", "the server no longer offers this addon"))?;
    let url = format!("{api}/v1/addons/registry/files/{}", entry.file);
    let bytes = tauri::async_runtime::spawn_blocking(move || download(&url, &token))
        .await
        .map_err(|e| err("io", e))?
        .map_err(|e| err("server", format!("could not download the addon: {e}")))?;
    let digest: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if !digest.eq_ignore_ascii_case(&entry.sha256) {
        return Err(err(
            "validation",
            "the downloaded package does not match what the server announced",
        ));
    }
    // From the bytes that were just checked: not through a file in a shared folder that another process could replace.
    let package = read_package_bytes(bytes)?;
    if package.manifest.id != entry.id || package.manifest.version != entry.version {
        return Err(err(
            "validation",
            "the package is not the addon the server announced",
        ));
    }
    install_package(&app, &state, &addons, package).await
}

// --------------------------------------------------------------------------
// Organization rules
// --------------------------------------------------------------------------

#[derive(Deserialize)]
struct Me {
    orgs: Vec<MeOrg>,
}
#[derive(Deserialize)]
struct MeOrg {
    id: String,
}
#[derive(Deserialize)]
struct OrgName {
    name: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Policy {
    mode: String,
    require_signed: bool,
    allowed: Vec<String>,
}

/// Asks the server what each of the person's organizations allows, keeps it, and applies it to the
/// installed addons. Without a sign-in the rules known from last time stay.
pub async fn refresh_rules(app: &AppHandle) -> Result<Vec<OrgRule>, CmdError> {
    let Some((api, token)) = session(app).await? else {
        return Ok(app.state::<Addons>().trust_snapshot().1);
    };
    let me: Me = get_json(&api, "/v1/me", &token).await?;
    let mut rules = Vec::new();
    for org in me.orgs {
        let policy: Policy = get_json(&api, &format!("/v1/orgs/{}/addons", org.id), &token).await?;
        if policy.mode == "open" && !policy.require_signed {
            continue;
        }
        let name: OrgName = get_json(&api, &format!("/v1/orgs/{}", org.id), &token).await?;
        rules.push(OrgRule {
            org_id: org.id,
            org_name: name.name,
            mode: policy.mode,
            require_signed: policy.require_signed,
            allowed: policy.allowed,
        });
    }
    let addons = app.state::<Addons>();
    addons.set_rules(rules.clone());
    save_state(app);
    // Something may now be allowed or refused.
    let state = app.state::<AppState>();
    if let Ok(records) = state.core.list_addons().await {
        for record in records {
            addons.reload(app, &record);
        }
    }
    Ok(rules)
}

/// What the person's organizations allow (asks the server; the last known rules when it cannot).
#[tauri::command]
#[specta::specta]
pub async fn addon_rules(app: AppHandle, addons: State<'_, Addons>) -> CmdResult<Vec<OrgRule>> {
    match refresh_rules(&app).await {
        Ok(rules) => Ok(rules),
        Err(e) => {
            eprintln!("addon rules: {}", e.message);
            Ok(addons.trust_snapshot().1)
        }
    }
}

/// Keeps the rules and publishers fresh in the background.
pub fn spawn(app: AppHandle) {
    load_state(&app);
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(20)).await;
            if let Err(e) = refresh_rules(&app).await {
                if e.code != "signed_out" {
                    eprintln!("addon rules: {}", e.message);
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(30 * 60)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(mode: &str, signed: bool, allowed: &[&str]) -> OrgRule {
        OrgRule {
            org_id: "o".into(),
            org_name: "Acme".into(),
            mode: mode.into(),
            require_signed: signed,
            allowed: allowed.iter().map(|s| (*s).into()).collect(),
        }
    }
    fn trusted() -> SignatureCheck {
        SignatureCheck {
            status: SignatureStatus::ServerTrusted,
            key_id: Some("k".into()),
            signer: Some("Pub".into()),
            fingerprint: Some("f".into()),
        }
    }

    #[test]
    fn an_open_rule_allows_everything() {
        assert!(rule("open", false, &[])
            .refuses("a.b", &SignatureCheck::unsigned())
            .is_none());
    }

    #[test]
    fn an_allowlist_allows_only_what_is_on_it() {
        let r = rule("allowlist", false, &["com.acme.hours"]);
        assert!(r.refuses("COM.acme.hours", &SignatureCheck::unsigned()).is_none());
        assert!(r.refuses("com.other.x", &trusted()).unwrap().contains("Acme"));
    }

    #[test]
    fn required_signatures_must_be_trusted() {
        let r = rule("open", true, &[]);
        assert!(r.refuses("a.b", &trusted()).is_none());
        assert!(r.refuses("a.b", &SignatureCheck::unsigned()).is_some());
        let unknown = SignatureCheck {
            status: SignatureStatus::UnknownKey,
            key_id: Some("x".into()),
            signer: None,
            fingerprint: None,
        };
        assert!(r.refuses("a.b", &unknown).is_some());
    }

    #[test]
    fn the_strictest_organization_wins() {
        let rules = [rule("open", false, &[]), rule("allowlist", false, &["a.b"])];
        assert!(refused_by(&rules, "a.b", &SignatureCheck::unsigned()).is_none());
        assert!(refused_by(&rules, "c.d", &SignatureCheck::unsigned()).is_some());
        assert!(refused_by(&[], "c.d", &SignatureCheck::unsigned()).is_none());
    }

    #[test]
    fn publishers_with_a_bad_key_are_not_trusted() {
        let keys = trusted_keys(&[Publisher {
            key_id: "k".into(),
            name: "n".into(),
            public_key: "not a key".into(),
        }]);
        assert!(keys.is_empty());
    }

    #[test]
    fn vendor_keys_come_from_the_build_and_cannot_be_replaced_by_a_server() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../../packages/shared-fixtures/addons/vectors.json")).unwrap();
        let real_key = v["key"]["publicKey"].as_str().unwrap().to_string();
        let vendor = parse_builtin(&format!("acme-1=Acme={real_key}; broken ;x=y"));
        assert_eq!(vendor.len(), 1);
        assert_eq!(
            (vendor[0].key_id.as_str(), vendor[0].name.as_str()),
            ("acme-1", "Acme")
        );
        assert!(parse_builtin("").is_empty());
        // A server that lists another key under the vendor's key id is ignored for that id; its own ids are kept.
        let fake = Publisher {
            key_id: "acme-1".into(),
            name: "Evil".into(),
            public_key: real_key.clone(),
        };
        let local = Publisher {
            key_id: "local".into(),
            name: "Local".into(),
            public_key: real_key,
        };
        let keys = keys_of(&vendor, &[fake, local]);
        assert_eq!(keys.len(), 2);
        assert!(keys.iter().find(|k| k.key_id == "acme-1").unwrap().vendor);
        assert_eq!(keys.iter().find(|k| k.key_id == "acme-1").unwrap().name, "Acme");
        assert!(!keys.iter().find(|k| k.key_id == "local").unwrap().vendor);
    }

    #[test]
    fn the_signature_record_round_trips() {
        let dir = std::env::temp_dir().join(format!("zeima-sigrec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(installed_signature(&dir).status, SignatureStatus::Unsigned);
        write_signature(&dir, &trusted()).unwrap();
        assert_eq!(installed_signature(&dir), trusted());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
