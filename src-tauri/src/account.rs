//! The user's Zeima account (SPEC §7): which server to use, sign-in with
//! OpenID Connect (Authorization Code + PKCE in the system browser, loopback
//! redirect) and the access token for API calls.
//!
//! The flow is the one addons use for their accounts (`oauth.rs`); the
//! account is filed in the keychain under a fixed pseudo-addon id. The
//! refresh token never leaves the keychain; access tokens live in memory.

use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;
use zeima_core::addon::OAuthProvider;

use crate::addons::{Addons, HostServices};
use crate::commands::{CmdError, CmdResult};
use crate::oauth;
use crate::secrets::{entry_name, Keychain, SecretStore};
use crate::state::AppState;

/// Keychain namespace and token-cache key of the account.
pub(crate) const ACCOUNT_ID: &str = "zeima-account";
const PROVIDER_ID: &str = "zeima";
const SERVER_KEY: &str = "server_url";
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// `GET {server}/.well-known/zeima-server`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    pub api_url: String,
    pub issuer: String,
    pub client_id: String,
    #[serde(default)]
    pub api_versions: Vec<u32>,
    /// Oldest app version the server works with (`major.minor.patch`).
    #[serde(default)]
    pub min_client_version: Option<String>,
}

/// The parts of `{issuer}/.well-known/openid-configuration` we use.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OidcConfig {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
}

/// API versions this app speaks.
pub const API_VERSION: u32 = 1;

fn http_get(url: &str) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(HTTP_TIMEOUT))
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header("Accept", "application/json")
        .call()
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("{url} answered {status}"));
    }
    response.body_mut().read_to_string().map_err(|e| e.to_string())
}

/// Checks and normalizes what the user typed: `https://` (or `http://` for
/// localhost), no trailing slash.
pub fn normalize_server_url(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        // This machine has no certificate: a development server speaks plain HTTP.
        let host = trimmed.split(['/', ':']).next().unwrap_or_default();
        let scheme = if matches!(host, "localhost" | "127.0.0.1" | "[::1]") {
            "http"
        } else {
            "https"
        };
        format!("{scheme}://{trimmed}")
    };
    let url = url::Url::parse(&with_scheme).map_err(|_| "that is not a valid address".to_string())?;
    let host = url.host_str().ok_or("that is not a valid address")?;
    let local = matches!(host, "localhost" | "127.0.0.1" | "[::1]");
    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        return Err("the server address must start with https://".into());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("the server address must not contain a query or fragment".into());
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

/// `major.minor.patch` of a version; a pre-release suffix is ignored.
fn version_parts(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u64>());
    Some((it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?))
}

/// Whether this app is at least the version the server requires. An unreadable
/// requirement is not held against the app.
pub fn meets_minimum(app: &str, minimum: &str) -> bool {
    match (version_parts(app), version_parts(minimum)) {
        (Some(a), Some(m)) => a >= m,
        _ => true,
    }
}

/// Finds the issuer and endpoints for a server. Blocks.
pub fn discover(server_url: &str) -> Result<(Discovery, OidcConfig), String> {
    let body = http_get(&format!("{server_url}/.well-known/zeima-server"))
        .map_err(|e| format!("could not reach the server: {e}"))?;
    let discovery: Discovery =
        serde_json::from_str(&body).map_err(|_| "that address is not a Zeima server".to_string())?;
    if !discovery.api_versions.contains(&API_VERSION) {
        return Err("this server needs a newer version of the app".into());
    }
    if let Some(minimum) = &discovery.min_client_version {
        if !meets_minimum(env!("CARGO_PKG_VERSION"), minimum) {
            return Err(format!(
                "this server needs Zeima {minimum} or newer; please update the app"
            ));
        }
    }
    let issuer = discovery.issuer.trim_end_matches('/');
    let body = http_get(&format!("{issuer}/.well-known/openid-configuration"))
        .map_err(|e| format!("could not reach the sign-in service: {e}"))?;
    let oidc: OidcConfig =
        serde_json::from_str(&body).map_err(|e| format!("sign-in service configuration: {e}"))?;
    Ok((discovery, oidc))
}

/// The OAuth provider for the account.
pub fn provider(discovery: &Discovery, oidc: &OidcConfig) -> OAuthProvider {
    OAuthProvider {
        id: PROVIDER_ID.into(),
        name: "Zeima".into(),
        auth_url: oidc.authorization_endpoint.clone(),
        token_url: oidc.token_endpoint.clone(),
        client_id: discovery.client_id.clone(),
        client_secret: None,
        scopes: ["openid", "profile", "email", "offline_access"]
            .map(String::from)
            .to_vec(),
        auth_params: Default::default(),
        redirect_host: Some("127.0.0.1".into()),
    }
}

// --------------------------------------------------------------------------
// The signed-in user
// --------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountUser {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

#[derive(Deserialize)]
struct Claims {
    sub: String,
    email: Option<String>,
    name: Option<String>,
    preferred_username: Option<String>,
    #[serde(default)]
    groups: Vec<String>,
}

/// The groups a token lists (`/orgs/<org>/roles/…`, `/orgs/<org>/groups/…`).
pub fn groups_from_token(token: &str) -> Vec<String> {
    let claims = token.split('.').nth(1).and_then(|payload| {
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload.trim_end_matches('='))
            .ok()?;
        serde_json::from_slice::<Claims>(&bytes).ok()
    });
    claims.map(|c| c.groups).unwrap_or_default()
}

/// Reads who a token is for. The signature is not checked: the token came
/// straight from the token endpoint over TLS, and the server verifies it on
/// every API call. This is for display only.
pub fn user_from_token(token: &str) -> Option<AccountUser> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let claims: Claims = serde_json::from_slice(&bytes).ok()?;
    Some(AccountUser {
        id: claims.sub,
        email: claims.email,
        name: claims.name.or(claims.preferred_username),
    })
}

// --------------------------------------------------------------------------
// Commands
// --------------------------------------------------------------------------

#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    pub server_url: Option<String>,
    pub signed_in: bool,
    pub user: Option<AccountUser>,
}

fn err(code: &str, message: impl ToString) -> CmdError {
    CmdError::other(code, message)
}

async fn server_url(state: &AppState) -> CmdResult<Option<String>> {
    Ok(state.core.get_meta(SERVER_KEY).await?.filter(|s| !s.is_empty()))
}

/// A valid access token for the API, refreshed when needed; `None` if not
/// signed in (or the grant was revoked, which signs out). Blocks.
fn access_token_blocking(services: &HostServices, server: &str, now: i64) -> Result<Option<String>, String> {
    let (discovery, oidc) = discover(server)?;
    oauth::access_token(
        ACCOUNT_ID,
        &provider(&discovery, &oidc),
        services.secrets.as_ref(),
        &services.post_form,
        &services.tokens,
        now,
    )
}

#[tauri::command]
#[specta::specta]
pub async fn account_status(app: AppHandle, state: State<'_, AppState>) -> CmdResult<AccountStatus> {
    let server = server_url(&state).await?;
    let Some(server_url) = server.clone() else {
        return Ok(AccountStatus {
            server_url: None,
            signed_in: false,
            user: None,
        });
    };
    let services = HostServices::for_app(&app);
    let now = state.core.now_ms();
    // A refresh token in the keychain means signed in; the token fetch also
    // tells us who, and notices a revoked grant. An unreachable server
    // leaves the sign-in as it was.
    let name = entry_name(ACCOUNT_ID, &oauth::secret_name(PROVIDER_ID));
    let secrets = services.secrets.clone();
    let has_refresh = tauri::async_runtime::spawn_blocking(move || secrets.get(&name))
        .await
        .map_err(|e| err("io", e))?
        .map_err(|e| err("keychain", e))?
        .is_some();
    if !has_refresh {
        return Ok(AccountStatus {
            server_url: server,
            signed_in: false,
            user: None,
        });
    }
    let token =
        tauri::async_runtime::spawn_blocking(move || access_token_blocking(&services, &server_url, now))
            .await
            .map_err(|e| err("io", e))?;
    Ok(match token {
        Ok(Some(token)) => AccountStatus {
            server_url: server,
            signed_in: true,
            user: user_from_token(&token),
        },
        Ok(None) => AccountStatus {
            server_url: server,
            signed_in: false,
            user: None,
        },
        Err(_) => AccountStatus {
            server_url: server,
            signed_in: true,
            user: None,
        },
    })
}

/// Sets (or, with `None`, clears) the server. Checks that it is a Zeima
/// server first. Changing the server signs out.
#[tauri::command]
#[specta::specta]
pub async fn account_set_server(
    app: AppHandle,
    state: State<'_, AppState>,
    addons: State<'_, Addons>,
    url: Option<String>,
) -> CmdResult<AccountStatus> {
    let new = match url.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
        None => None,
        Some(input) => {
            let normalized = normalize_server_url(input).map_err(|e| err("validation", e))?;
            let probe = normalized.clone();
            tauri::async_runtime::spawn_blocking(move || discover(&probe))
                .await
                .map_err(|e| err("io", e))?
                .map_err(|e| err("server", e))?;
            Some(normalized)
        }
    };
    if new != server_url(&state).await? {
        sign_out_inner(&addons).await?;
        crate::license::clear(&app).await;
        crate::sync::reset_join(&app).await;
    }
    state
        .core
        .set_meta(SERVER_KEY, new.as_deref().unwrap_or(""))
        .await?;
    account_status(app, state).await
}

/// Signs in in the system browser. Resolves when done, failed, timed out
/// (5 minutes) or cancelled (error code `cancelled`).
#[tauri::command]
#[specta::specta]
pub async fn account_sign_in(
    app: AppHandle,
    state: State<'_, AppState>,
    addons: State<'_, Addons>,
) -> CmdResult<AccountStatus> {
    let server = server_url(&state)
        .await?
        .ok_or_else(|| err("validation", "choose a server first"))?;
    let probe = server.clone();
    let (discovery, oidc) = tauri::async_runtime::spawn_blocking(move || discover(&probe))
        .await
        .map_err(|e| err("io", e))?
        .map_err(|e| err("server", e))?;
    let provider = provider(&discovery, &oidc);

    let services = HostServices::for_app(&app);
    let cancel = addons.begin_sign_in(ACCOUNT_ID);
    let pages = crate::commands_addons::sign_in_pages(state.settings().language());
    let opener = app.clone();
    let result = oauth::connect(
        ACCOUNT_ID,
        &provider,
        services.secrets.clone(),
        services.post_form.clone(),
        &services.tokens,
        state.core.now_ms(),
        move |url| {
            opener
                .opener()
                .open_url(url, None::<&str>)
                .map_err(|e| format!("could not open the browser: {e}"))
        },
        cancel,
        &pages,
    )
    .await;
    addons.end_sign_in(ACCOUNT_ID);
    match result {
        Ok(()) => {}
        Err(e) if e == "cancelled" => return Err(err("cancelled", e)),
        Err(e) => return Err(err("oauth", e)),
    }
    account_status(app, state).await
}

#[tauri::command]
#[specta::specta]
pub async fn account_cancel(addons: State<'_, Addons>) -> CmdResult<()> {
    addons.cancel_sign_in(ACCOUNT_ID);
    Ok(())
}

async fn sign_out_inner(addons: &Addons) -> CmdResult<()> {
    let name = entry_name(ACCOUNT_ID, &oauth::secret_name(PROVIDER_ID));
    tauri::async_runtime::spawn_blocking(move || Keychain.delete(&name))
        .await
        .map_err(|e| err("io", e))?
        .map_err(|e| err("keychain", e))?;
    addons.tokens.forget(ACCOUNT_ID, Some(PROVIDER_ID));
    Ok(())
}

/// Forgets the sign-in on this device. The server session stays until it
/// expires; revoking it from the account page signs out everywhere.
#[tauri::command]
#[specta::specta]
pub async fn account_sign_out(
    app: AppHandle,
    state: State<'_, AppState>,
    addons: State<'_, Addons>,
) -> CmdResult<AccountStatus> {
    sign_out_inner(&addons).await?;
    crate::license::clear(&app).await;
    crate::sync::reset_join(&app).await;
    account_status(app, state).await
}

/// A valid access token for API calls, or `None` when signed out or no
/// server is chosen.
pub(crate) async fn current_token(app: &AppHandle) -> Result<Option<(String, String)>, String> {
    let state = app.state::<AppState>();
    let Some(server) = state
        .core
        .get_meta(SERVER_KEY)
        .await
        .map_err(|e| e.to_string())?
        .filter(|s| !s.is_empty())
    else {
        return Ok(None);
    };
    let services = HostServices::for_app(app);
    let now = state.core.now_ms();
    let probe = server.clone();
    let token = tauri::async_runtime::spawn_blocking(move || access_token_blocking(&services, &probe, now))
        .await
        .map_err(|e| e.to_string())??;
    Ok(token.map(|t| (server, t)))
}

/// A valid access token for API calls, or `None` when signed out.
#[tauri::command]
#[specta::specta]
pub async fn account_access_token(app: AppHandle) -> CmdResult<Option<String>> {
    Ok(current_token(&app)
        .await
        .map_err(|e| err("server", e))?
        .map(|(_, token)| token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_addresses_are_normalized() {
        assert_eq!(
            normalize_server_url("zeima.example.com").unwrap(),
            "https://zeima.example.com"
        );
        assert_eq!(
            normalize_server_url("  https://zeima.example.com/ ").unwrap(),
            "https://zeima.example.com"
        );
        assert_eq!(
            normalize_server_url("http://localhost:3000/").unwrap(),
            "http://localhost:3000"
        );
        assert_eq!(
            normalize_server_url("https://z.example/api").unwrap(),
            "https://z.example/api"
        );
    }

    #[test]
    fn plain_http_is_only_for_this_machine() {
        assert!(normalize_server_url("http://zeima.example.com").is_err());
        assert!(normalize_server_url("http://127.0.0.1:3000").is_ok());
    }

    #[test]
    fn bad_addresses_are_rejected() {
        assert!(normalize_server_url("").is_err());
        assert!(normalize_server_url("https://z.example/?x=1").is_err());
        assert!(normalize_server_url("https://z.example/#top").is_err());
    }

    #[test]
    fn the_minimum_version_is_compared_numerically() {
        assert!(meets_minimum("1.2.0", "1.2.0"));
        assert!(meets_minimum("1.10.0", "1.9.9"));
        assert!(!meets_minimum("1.9.9", "1.10.0"));
        assert!(!meets_minimum("0.9.9", "1.0.0"));
        // A pre-release counts as its release; unreadable requirements are ignored.
        assert!(meets_minimum("1.1.0-alpha.0", "1.1.0"));
        assert!(meets_minimum("1.0.0", "soon"));
    }

    #[test]
    fn the_provider_uses_the_discovered_endpoints() {
        let discovery = Discovery {
            api_url: "http://localhost:3000".into(),
            issuer: "http://localhost:8080/realms/zeima".into(),
            client_id: "zeima-desktop".into(),
            api_versions: vec![1],
            min_client_version: None,
        };
        let oidc = OidcConfig {
            authorization_endpoint: "http://localhost:8080/realms/zeima/protocol/openid-connect/auth".into(),
            token_endpoint: "http://localhost:8080/realms/zeima/protocol/openid-connect/token".into(),
        };
        let p = provider(&discovery, &oidc);
        assert_eq!(p.client_id, "zeima-desktop");
        assert_eq!(p.auth_url, oidc.authorization_endpoint);
        assert!(p.client_secret.is_none());
        assert!(p.scopes.contains(&"offline_access".to_string()));
    }

    fn jwt(payload: &str) -> String {
        let enc = |s: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s);
        format!("{}.{}.sig", enc("{\"alg\":\"RS256\"}"), enc(payload))
    }

    #[test]
    fn the_user_is_read_from_the_token() {
        let token =
            jwt(r#"{"sub":"u1","email":"ada@example.com","name":"Ada Lovelace","preferred_username":"ada"}"#);
        let user = user_from_token(&token).unwrap();
        assert_eq!(user.id, "u1");
        assert_eq!(user.email.as_deref(), Some("ada@example.com"));
        assert_eq!(user.name.as_deref(), Some("Ada Lovelace"));
    }

    #[test]
    fn the_username_stands_in_for_a_missing_name() {
        let user = user_from_token(&jwt(r#"{"sub":"u1","preferred_username":"ada"}"#)).unwrap();
        assert_eq!(user.name.as_deref(), Some("ada"));
        assert_eq!(user.email, None);
    }

    #[test]
    fn garbage_tokens_give_no_user() {
        assert!(user_from_token("not a token").is_none());
        assert!(user_from_token(&jwt("{}")).is_none());
    }
}
