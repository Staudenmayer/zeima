//! OAuth 2 for addon accounts (`[[oauth]]` in `addon.toml`): authorization
//! code flow with PKCE in the system browser and a loopback redirect
//! (RFC 8252). The refresh token is kept in the keychain under
//! `oauth:<provider>`; access tokens only in memory. Addons never see the
//! refresh token.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use zeima_core::addon::OAuthProvider;

use crate::secrets::{entry_name, SecretStore};

/// How long the browser sign-in may take.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Access tokens are renewed this long before they expire.
const EXPIRY_MARGIN_MS: i64 = 60_000;
/// Assumed lifetime when the provider does not say.
const DEFAULT_EXPIRES_IN_S: i64 = 3_600;
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

/// The `addon_secret` name of a provider's refresh token.
pub fn secret_name(provider_id: &str) -> String {
    format!("oauth:{provider_id}")
}

/// POSTs a form; returns status and body. Injected so tests need no network.
pub type PostForm = Arc<dyn Fn(&str, &[(String, String)]) -> Result<(u16, String), String> + Send + Sync>;

/// `PostForm` over HTTPS with ureq.
pub fn http_post_form() -> PostForm {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(HTTP_TIMEOUT))
        .build()
        .into();
    Arc::new(move |url, form| {
        let mut response = agent
            .post(url)
            .header("Accept", "application/json")
            .send_form(form.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let body = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
        Ok((status, body))
    })
}

// --------------------------------------------------------------------------
// PKCE and the authorization request
// --------------------------------------------------------------------------

/// `bytes` random bytes, base64url without padding.
fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("OS random numbers");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

/// S256 code challenge of a verifier.
fn challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn redirect_uri(provider: &OAuthProvider, port: u16) -> String {
    let host = provider.redirect_host.as_deref().unwrap_or("127.0.0.1");
    format!("http://{host}:{port}")
}

fn authorize_url(
    provider: &OAuthProvider,
    redirect_uri: &str,
    state: &str,
    verifier: &str,
) -> Result<String, String> {
    let mut url = url::Url::parse(&provider.auth_url).map_err(|e| format!("auth_url: {e}"))?;
    {
        let mut q = url.query_pairs_mut();
        // Provider extras first, so they cannot replace the flow's own.
        for (k, v) in &provider.auth_params {
            q.append_pair(k, v);
        }
        q.append_pair("response_type", "code")
            .append_pair("client_id", &provider.client_id)
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("state", state)
            .append_pair("code_challenge", &challenge(verifier))
            .append_pair("code_challenge_method", "S256");
        if !provider.scopes.is_empty() {
            q.append_pair("scope", &provider.scopes.join(" "));
        }
    }
    Ok(url.into())
}

// --------------------------------------------------------------------------
// Loopback redirect
// --------------------------------------------------------------------------

/// Pages the browser shows after the redirect.
pub struct Pages {
    pub success: String,
    pub failure: String,
}

async fn respond(stream: &mut tokio::net::TcpStream, status: &str, html: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{html}",
        html.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// Query parameters of the request line `GET /path?query HTTP/1.1`.
fn request_query(head: &str) -> Option<HashMap<String, String>> {
    let target = head.lines().next()?.strip_prefix("GET ")?.split(' ').next()?;
    let query = target.split_once('?').map_or("", |(_, q)| q);
    Some(
        url::form_urlencoded::parse(query.as_bytes())
            .into_owned()
            .collect(),
    )
}

/// Waits for the provider's redirect with `state`; returns the code. Other
/// requests (favicon, stale tabs) are answered and ignored.
async fn wait_for_code(listener: &TcpListener, state: &str, pages: &Pages) -> Result<String, String> {
    loop {
        let (mut stream, _) = listener.accept().await.map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 8 * 1024];
        let mut len = 0;
        // Only the request head is needed.
        while len < buf.len() {
            match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf[len..])).await {
                Ok(Ok(0)) | Err(_) | Ok(Err(_)) => break,
                Ok(Ok(n)) => len += n,
            }
            if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let head = String::from_utf8_lossy(&buf[..len]);
        let Some(params) = request_query(&head) else {
            respond(&mut stream, "400 Bad Request", "").await;
            continue;
        };
        if params.get("state").map(String::as_str) != Some(state) {
            respond(&mut stream, "404 Not Found", "").await;
            continue;
        }
        if let Some(code) = params.get("code") {
            respond(&mut stream, "200 OK", &pages.success).await;
            return Ok(code.clone());
        }
        respond(&mut stream, "200 OK", &pages.failure).await;
        let error = params.get("error").map_or("no code", String::as_str);
        return Err(match params.get("error_description") {
            Some(d) => format!("{error}: {d}"),
            None => error.to_string(),
        });
    }
}

// --------------------------------------------------------------------------
// Token endpoint
// --------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
}

#[derive(Debug, PartialEq)]
pub enum TokenError {
    /// The grant is no longer valid (`invalid_grant`): sign in again.
    Revoked(String),
    Other(String),
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Revoked(m) | Self::Other(m) => f.write_str(m),
        }
    }
}

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

fn token_request(
    post: &PostForm,
    provider: &OAuthProvider,
    mut form: Vec<(String, String)>,
) -> Result<TokenResponse, TokenError> {
    form.push(("client_id".into(), provider.client_id.clone()));
    if let Some(secret) = &provider.client_secret {
        form.push(("client_secret".into(), secret.clone()));
    }
    let (status, body) = post(&provider.token_url, &form).map_err(TokenError::Other)?;
    if (200..300).contains(&status) {
        return serde_json::from_str(&body).map_err(|e| TokenError::Other(format!("token response: {e}")));
    }
    match serde_json::from_str::<ErrorBody>(&body) {
        Ok(e) => {
            let message = match e.error_description {
                Some(d) => format!("{}: {d}", e.error),
                None => e.error.clone(),
            };
            Err(if e.error == "invalid_grant" {
                TokenError::Revoked(message)
            } else {
                TokenError::Other(message)
            })
        }
        Err(_) => Err(TokenError::Other(format!("token endpoint answered {status}"))),
    }
}

fn exchange_code(
    post: &PostForm,
    provider: &OAuthProvider,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse, TokenError> {
    token_request(
        post,
        provider,
        vec![
            ("grant_type".into(), "authorization_code".into()),
            ("code".into(), code.into()),
            ("redirect_uri".into(), redirect_uri.into()),
            ("code_verifier".into(), verifier.into()),
        ],
    )
}

fn refresh(
    post: &PostForm,
    provider: &OAuthProvider,
    refresh_token: &str,
) -> Result<TokenResponse, TokenError> {
    let mut form = vec![
        ("grant_type".to_string(), "refresh_token".to_string()),
        ("refresh_token".to_string(), refresh_token.to_string()),
    ];
    if !provider.scopes.is_empty() {
        form.push(("scope".into(), provider.scopes.join(" ")));
    }
    token_request(post, provider, form)
}

// --------------------------------------------------------------------------
// Connecting and access tokens
// --------------------------------------------------------------------------

/// Access tokens in memory: (addon, provider) → (token, valid until ms).
#[derive(Default)]
pub struct TokenCache {
    tokens: Mutex<HashMap<(String, String), (String, i64)>>,
    /// One refresh at a time. Refresh tokens are rotated (each is good once):
    /// when several requests find the cache empty together (at startup) and
    /// all refresh with the same token, all but the first are refused and the
    /// account would be disconnected.
    refreshing: Mutex<()>,
}

impl TokenCache {
    fn key(addon_id: &str, provider_id: &str) -> (String, String) {
        (addon_id.to_string(), provider_id.to_string())
    }

    fn get(&self, addon_id: &str, provider_id: &str, now: i64) -> Option<String> {
        let map = self.tokens.lock().unwrap_or_else(|e| e.into_inner());
        map.get(&Self::key(addon_id, provider_id))
            .filter(|(_, until)| *until > now)
            .map(|(token, _)| token.clone())
    }

    fn put(&self, addon_id: &str, provider_id: &str, token: &TokenResponse, now: i64) {
        let lifetime = token.expires_in.unwrap_or(DEFAULT_EXPIRES_IN_S) * 1000;
        let until = now + (lifetime - EXPIRY_MARGIN_MS).max(0);
        self.tokens.lock().unwrap_or_else(|e| e.into_inner()).insert(
            Self::key(addon_id, provider_id),
            (token.access_token.clone(), until),
        );
    }

    pub fn forget(&self, addon_id: &str, provider_id: Option<&str>) {
        self.tokens
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(a, p), _| !(a == addon_id && provider_id.is_none_or(|id| id == p)));
    }
}

/// Signs in through the browser and stores the refresh token in the
/// keychain. `open` opens the URL in the browser; `cancel` ends the wait.
#[allow(clippy::too_many_arguments)]
pub async fn connect(
    addon_id: &str,
    provider: &OAuthProvider,
    secrets: Arc<dyn SecretStore>,
    post: PostForm,
    cache: &TokenCache,
    now: i64,
    open: impl FnOnce(&str) -> Result<(), String>,
    cancel: tokio::sync::oneshot::Receiver<()>,
    pages: &Pages,
) -> Result<(), String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| format!("could not open the sign-in port: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = redirect_uri(provider, port);
    let state = random_token(16);
    let verifier = random_token(48);
    open(&authorize_url(provider, &redirect, &state, &verifier)?)?;

    let code = tokio::select! {
        result = tokio::time::timeout(SIGN_IN_TIMEOUT, wait_for_code(&listener, &state, pages)) => {
            result.map_err(|_| "the sign-in timed out".to_string())??
        }
        _ = cancel => return Err("cancelled".into()),
    };
    drop(listener);

    let name = entry_name(addon_id, &secret_name(&provider.id));
    let provider_owned = provider.clone();
    let token = tauri::async_runtime::spawn_blocking(move || {
        let token =
            exchange_code(&post, &provider_owned, &code, &verifier, &redirect).map_err(|e| e.to_string())?;
        let refresh_token = token.refresh_token.as_deref().ok_or(
            "the provider sent no refresh token (scope offline_access, or for Google access_type = offline)",
        )?;
        secrets.set(&name, refresh_token)?;
        Ok::<_, String>(token)
    })
    .await
    .map_err(|e| e.to_string())??;
    cache.put(addon_id, &provider.id, &token, now);
    Ok(())
}

/// A valid access token, refreshed with the stored refresh token when
/// needed. `None` if the account is not connected or the provider revoked
/// the grant (the refresh token is then deleted). Blocks.
pub fn access_token(
    addon_id: &str,
    provider: &OAuthProvider,
    secrets: &dyn SecretStore,
    post: &PostForm,
    cache: &TokenCache,
    now: i64,
) -> Result<Option<String>, String> {
    if let Some(token) = cache.get(addon_id, &provider.id, now) {
        return Ok(Some(token));
    }
    let _one_at_a_time = cache.refreshing.lock().unwrap_or_else(|e| e.into_inner());
    // Whoever was ahead of us has refreshed already.
    if let Some(token) = cache.get(addon_id, &provider.id, now) {
        return Ok(Some(token));
    }
    let name = entry_name(addon_id, &secret_name(&provider.id));
    // A second round only when another instance rotated the token meanwhile.
    for _ in 0..2 {
        let Some(refresh_token) = secrets.get(&name)? else {
            return Ok(None);
        };
        match refresh(post, provider, &refresh_token) {
            Ok(token) => {
                // The new access token first: if keeping the rotated refresh
                // token fails, this session still works.
                cache.put(addon_id, &provider.id, &token, now);
                // Providers may rotate the refresh token; the old one is dead then.
                if let Some(new) = token.refresh_token.as_deref().filter(|t| *t != refresh_token) {
                    if secrets.set(&name, new).is_err() {
                        secrets.set(&name, new)?;
                    }
                }
                return Ok(Some(token.access_token));
            }
            Err(TokenError::Revoked(_)) => {
                // Only the token we used is dead; a newer one (stored by
                // another instance) is not ours to delete.
                if secrets.get(&name)?.as_deref() == Some(refresh_token.as_str()) {
                    secrets.delete(&name)?;
                    return Ok(None);
                }
            }
            Err(TokenError::Other(e)) => return Err(format!("{}: {e}", provider.name)),
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemorySecrets;

    fn provider() -> OAuthProvider {
        OAuthProvider {
            id: "ms".into(),
            name: "Microsoft 365".into(),
            auth_url: "https://login.example.com/authorize?tenant=common".into(),
            token_url: "https://login.example.com/token".into(),
            client_id: "client-1".into(),
            client_secret: None,
            scopes: vec!["offline_access".into(), "Calendars.Read".into()],
            auth_params: [("prompt".to_string(), "select_account".to_string())].into(),
            redirect_host: None,
        }
    }

    fn pages() -> Pages {
        Pages {
            success: "ok".into(),
            failure: "failed".into(),
        }
    }

    /// Records requests and answers with `responses` in order.
    /// Forms sent to the fake token endpoint.
    type Sent = Arc<Mutex<Vec<HashMap<String, String>>>>;

    fn fake_post(responses: Vec<(u16, &str)>) -> (PostForm, Sent) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let responses = Mutex::new(
            responses
                .into_iter()
                .map(|(s, b)| (s, b.to_string()))
                .collect::<Vec<_>>(),
        );
        let post: PostForm = Arc::new(move |url, form| {
            assert_eq!(url, "https://login.example.com/token");
            log.lock().unwrap().push(form.iter().cloned().collect());
            Ok(responses.lock().unwrap().remove(0))
        });
        (post, seen)
    }

    #[test]
    fn pkce_challenge_matches_rfc_7636() {
        // RFC 7636, appendix B.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(random_token(48).len(), 64);
    }

    #[test]
    fn authorize_url_carries_the_flow_parameters() {
        let url = authorize_url(&provider(), "http://127.0.0.1:4711", "st", "verifier").unwrap();
        let parsed = url::Url::parse(&url).unwrap();
        let q: HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(q["tenant"], "common");
        assert_eq!(q["prompt"], "select_account");
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["client_id"], "client-1");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:4711");
        assert_eq!(q["state"], "st");
        assert_eq!(q["code_challenge"], challenge("verifier"));
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["scope"], "offline_access Calendars.Read");
    }

    /// The browser side: follows the redirect like a provider would.
    async fn browser(url: String, code: Option<&'static str>) {
        let parsed = url::Url::parse(&url).unwrap();
        let q: HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        let redirect = url::Url::parse(&q["redirect_uri"]).unwrap();
        let addr = format!("127.0.0.1:{}", redirect.port().unwrap());
        let get = |query: String| {
            let addr = addr.clone();
            async move {
                let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
                s.write_all(format!("GET /?{query} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                    .await
                    .unwrap();
                let mut out = String::new();
                s.read_to_string(&mut out).await.unwrap();
                out
            }
        };
        // A stray request (other tab, wrong state) is ignored.
        assert!(get("code=evil&state=other".into())
            .await
            .starts_with("HTTP/1.1 404"));
        let answer = match code {
            Some(code) => get(format!("code={code}&state={}", q["state"])).await,
            None => get(format!("error=access_denied&state={}", q["state"])).await,
        };
        assert!(answer.starts_with("HTTP/1.1 200"));
    }

    #[tokio::test]
    async fn connect_runs_the_flow_and_stores_the_refresh_token() {
        let secrets = Arc::new(MemorySecrets::default());
        let (post, seen) = fake_post(vec![(
            200,
            r#"{"access_token":"at-1","refresh_token":"rt-1","expires_in":3600}"#,
        )]);
        let cache = TokenCache::default();
        let (_tx, cancel) = tokio::sync::oneshot::channel();
        let mut url = String::new();
        let open = |u: &str| {
            url = u.to_string();
            tokio::spawn(browser(u.to_string(), Some("the-code")));
            Ok(())
        };
        connect(
            "dev.a",
            &provider(),
            secrets.clone(),
            post,
            &cache,
            1_000,
            open,
            cancel,
            &pages(),
        )
        .await
        .unwrap();

        let form = &seen.lock().unwrap()[0];
        assert_eq!(form["grant_type"], "authorization_code");
        assert_eq!(form["code"], "the-code");
        assert_eq!(challenge(&form["code_verifier"]), {
            let q: HashMap<_, _> = url::Url::parse(&url)
                .unwrap()
                .query_pairs()
                .into_owned()
                .collect();
            q["code_challenge"].clone()
        });
        assert!(form["redirect_uri"].starts_with("http://127.0.0.1:"));
        assert_eq!(
            secrets.get("addon:dev.a:oauth:ms").unwrap().as_deref(),
            Some("rt-1")
        );
        assert_eq!(cache.get("dev.a", "ms", 2_000).as_deref(), Some("at-1"));
    }

    #[tokio::test]
    async fn a_denied_sign_in_stores_nothing() {
        let secrets = Arc::new(MemorySecrets::default());
        let (post, seen) = fake_post(vec![]);
        let (_tx, cancel) = tokio::sync::oneshot::channel();
        let open = |u: &str| {
            tokio::spawn(browser(u.to_string(), None));
            Ok(())
        };
        let err = connect(
            "dev.a",
            &provider(),
            secrets.clone(),
            post,
            &TokenCache::default(),
            0,
            open,
            cancel,
            &pages(),
        )
        .await
        .unwrap_err();
        assert_eq!(err, "access_denied");
        assert!(seen.lock().unwrap().is_empty());
        assert!(secrets.0.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn connect_can_be_cancelled() {
        let (post, _) = fake_post(vec![]);
        let (tx, cancel) = tokio::sync::oneshot::channel();
        tx.send(()).unwrap();
        let err = connect(
            "dev.a",
            &provider(),
            Arc::new(MemorySecrets::default()),
            post,
            &TokenCache::default(),
            0,
            |_| Ok(()),
            cancel,
            &pages(),
        )
        .await
        .unwrap_err();
        assert_eq!(err, "cancelled");
    }

    #[test]
    fn access_tokens_are_cached_refreshed_and_dropped_when_revoked() {
        let secrets = MemorySecrets::default();
        let cache = TokenCache::default();
        let p = provider();
        let (post, seen) = fake_post(vec![
            (
                200,
                r#"{"access_token":"at-2","refresh_token":"rt-2","expires_in":120}"#,
            ),
            (400, r#"{"error":"invalid_grant","error_description":"expired"}"#),
        ]);
        // Not connected.
        assert_eq!(
            access_token("dev.a", &p, &secrets, &post, &cache, 0).unwrap(),
            None
        );

        secrets.set("addon:dev.a:oauth:ms", "rt-1").unwrap();
        assert_eq!(
            access_token("dev.a", &p, &secrets, &post, &cache, 0)
                .unwrap()
                .as_deref(),
            Some("at-2")
        );
        assert_eq!(seen.lock().unwrap()[0]["refresh_token"], "rt-1");
        // Rotated refresh token is kept.
        assert_eq!(
            secrets.get("addon:dev.a:oauth:ms").unwrap().as_deref(),
            Some("rt-2")
        );
        // Cached until a minute before it expires (120 s).
        assert_eq!(
            access_token("dev.a", &p, &secrets, &post, &cache, 59_000)
                .unwrap()
                .as_deref(),
            Some("at-2")
        );
        assert_eq!(seen.lock().unwrap().len(), 1);
        // Expired and the grant revoked: disconnected.
        assert_eq!(
            access_token("dev.a", &p, &secrets, &post, &cache, 61_000).unwrap(),
            None
        );
        assert_eq!(seen.lock().unwrap()[1]["refresh_token"], "rt-2");
        assert_eq!(secrets.get("addon:dev.a:oauth:ms").unwrap(), None);
    }

    /// Many requests at once with an empty cache (the app's start): the refresh
    /// token is good once, so exactly one of them may use it.
    #[test]
    fn parallel_requests_refresh_once_and_stay_connected() {
        let secrets = Arc::new(MemorySecrets::default());
        secrets.set("addon:dev.a:oauth:ms", "rt-1").unwrap();
        let cache = Arc::new(TokenCache::default());
        let used = Arc::new(Mutex::new(Vec::<String>::new()));
        let log = used.clone();
        let post: PostForm = Arc::new(move |_, form| {
            let token = form.iter().find(|(k, _)| k == "refresh_token").unwrap().1.clone();
            let mut used = log.lock().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
            // Rotation: each refresh token works once.
            if used.contains(&token) || token != "rt-1" && token != "rt-2" {
                return Ok((400, r#"{"error":"invalid_grant"}"#.to_string()));
            }
            used.push(token);
            Ok((
                200,
                r#"{"access_token":"at-2","refresh_token":"rt-2","expires_in":300}"#.to_string(),
            ))
        });
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let (secrets, cache, post) = (secrets.clone(), cache.clone(), post.clone());
                std::thread::spawn(move || {
                    access_token("dev.a", &provider(), secrets.as_ref(), &post, &cache, 0)
                })
            })
            .collect();
        for t in threads {
            assert_eq!(t.join().unwrap().unwrap().as_deref(), Some("at-2"));
        }
        assert_eq!(used.lock().unwrap().len(), 1);
        assert_eq!(
            secrets.get("addon:dev.a:oauth:ms").unwrap().as_deref(),
            Some("rt-2")
        );
    }

    /// An instance that lost the race to rotate must not delete the winner's token.
    #[test]
    fn a_revoked_old_token_does_not_delete_a_newer_one() {
        let secrets = MemorySecrets::default();
        secrets.set("addon:dev.a:oauth:ms", "rt-1").unwrap();
        let swapped = Arc::new(Mutex::new(false));
        let handle = swapped.clone();
        let store = Arc::new(secrets);
        let inner = store.clone();
        // The first refresh is refused, and meanwhile "another instance" stored rt-9.
        let post: PostForm = Arc::new(move |_, form| {
            let token = form.iter().find(|(k, _)| k == "refresh_token").unwrap().1.clone();
            if token == "rt-1" {
                inner.set("addon:dev.a:oauth:ms", "rt-9").unwrap();
                *handle.lock().unwrap() = true;
                return Ok((400, r#"{"error":"invalid_grant"}"#.to_string()));
            }
            Ok((200, r#"{"access_token":"at-9","expires_in":300}"#.to_string()))
        });
        let got = access_token(
            "dev.a",
            &provider(),
            store.as_ref(),
            &post,
            &TokenCache::default(),
            0,
        )
        .unwrap();
        assert_eq!(got.as_deref(), Some("at-9"));
        assert_eq!(
            store.get("addon:dev.a:oauth:ms").unwrap().as_deref(),
            Some("rt-9")
        );
        assert!(*swapped.lock().unwrap());
    }

    #[test]
    fn other_token_errors_keep_the_account() {
        let secrets = MemorySecrets::default();
        secrets.set("addon:dev.a:oauth:ms", "rt-1").unwrap();
        let (post, _) = fake_post(vec![(503, "busy")]);
        let err = access_token("dev.a", &provider(), &secrets, &post, &TokenCache::default(), 0).unwrap_err();
        assert_eq!(err, "Microsoft 365: token endpoint answered 503");
        assert!(secrets.get("addon:dev.a:oauth:ms").unwrap().is_some());
    }
}
