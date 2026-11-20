//! Organization administration (SPEC §5.9): the admin pages call the
//! server's `/v1/orgs/…` routes through this narrow proxy, so the access
//! token stays in the Rust side and the web view needs no network access.
//! Anything outside `/v1/orgs` and `/v1/me` is refused.

use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::account::{self, discover, ACCOUNT_ID};
use crate::addons::Addons;
use crate::commands::{CmdError, CmdResult};

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const METHODS: [&str; 5] = ["GET", "POST", "PATCH", "PUT", "DELETE"];

fn err(code: &str, message: impl ToString) -> CmdError {
    CmdError::other(code, message)
}

/// Every `/`-separated segment (query string aside) is non-empty and not `.`: `/v1/orgs/./x` and `/v1/orgs/x/` name
/// the same resource as `/v1/orgs/x` for the server, but would slip past the exact-shape check of the confirmation.
fn canonical_segments(rest: &str) -> bool {
    let rest = rest.split('?').next().unwrap_or("");
    rest.split('/').all(|s| !s.is_empty() && s != ".")
}

/// What the app does with a request below `/v1/orgs`: `None` when no such route exists, `Some(None)` when it is sent
/// as is, `Some(Some(text))` when the app asks the person first (own dialog text, English / German). Every route is
/// listed with exactly the methods the server has for it, so the web view cannot reach any other combination.
fn org_route(method: &str, segments: &[&str]) -> Option<Option<&'static str>> {
    const DELETE_ORG: &str = "Delete this organization, its memberships and its shared data? This cannot be undone.\n\nDiese Organisation samt Mitgliedschaften und gemeinsamen Daten löschen? Das lässt sich nicht rückgängig machen.";
    const MEMBER: &str = "Change this member's role or deactivate their account?\n\nRolle dieses Mitglieds ändern oder das Konto deaktivieren?";
    const REMOVE_MEMBER: &str =
        "Remove this member from the organization?\n\nDieses Mitglied aus der Organisation entfernen?";
    const DELETE_ROLE: &str = "Delete this role? Members who hold it lose its permissions.\n\nDiese Rolle löschen? Mitglieder, die sie haben, verlieren ihre Rechte.";
    const ADDONS: &str = "Change which add-ons this organization allows?\n\nÄndern, welche Add-ons diese Organisation erlaubt?";
    const DELETE_SSO: &str = "Remove the single sign-on setup of this organization?\n\nDie Single-Sign-on-Einrichtung dieser Organisation entfernen?";
    const SSO: &str = "Change the single sign-on setup of this organization?\n\nDie Single-Sign-on-Einrichtung dieser Organisation ändern?";
    const DELETE_TOKEN: &str = "Revoke this provisioning token?\n\nDieses Provisionierungs-Token widerrufen?";
    let open = Some(None);
    let ask = |t| Some(Some(t));
    match (segments, method) {
        ([], "GET" | "POST") => open,
        ([_], "GET") => open,
        ([_], "DELETE") => ask(DELETE_ORG),
        (
            [_, "members" | "invitations" | "groups" | "people" | "jobs" | "directory" | "roles" | "audit"],
            "GET",
        ) => open,
        ([_, "presence"], "GET" | "PUT") => open,
        ([_, "scim" | "addons"], "GET") => open,
        ([_, "addons"], "PUT") => ask(ADDONS),
        ([_, "sso"], "GET") => open,
        ([_, "sso"], "PUT") => ask(SSO),
        ([_, "sso"], "DELETE") => ask(DELETE_SSO),
        ([_, "invitations" | "groups" | "jobs" | "roles"], "POST") => open,
        ([_, "reconcile" | "freebusy"], "POST") => open,
        ([_, "rooms", "bookings"], "POST") => open,
        ([_, "scim", "tokens"], "POST") => open,
        ([_, "scim", "tokens", _], "DELETE") => ask(DELETE_TOKEN),
        ([_, "members", _], "PATCH") => ask(MEMBER),
        ([_, "members", _], "DELETE") => ask(REMOVE_MEMBER),
        ([_, "members", _, "job"], "PUT") => open,
        ([_, "invitations" | "groups" | "jobs", _], "PATCH" | "DELETE") => open,
        ([_, "roles", _], "PATCH") => open,
        ([_, "roles", _], "DELETE") => ask(DELETE_ROLE),
        ([_, "groups", _, "members", _], "PUT" | "DELETE") => open,
        _ => None,
    }
}

fn org_segments(path: &str) -> Option<Vec<&str>> {
    let rest = path.strip_prefix("/v1/orgs")?;
    let rest = rest.split('?').next().unwrap_or("");
    if rest.is_empty() {
        return Some(Vec::new());
    }
    let rest = rest.strip_prefix('/')?;
    canonical_segments(rest).then(|| rest.split('/').collect())
}

/// Whether the app may send this request: a known route with a method it has, and a plain path (`/v1/me`, the
/// company directory, or below `/v1/orgs`), without tricks.
pub fn allowed(method: &str, path: &str) -> bool {
    let plain = path.is_ascii()
        && !path.contains("..")
        && !path.contains("//")
        && !path.contains('\\')
        // Percent-encoded dots and slashes (%2e%2e, %2f) would pass the checks above and be decoded by the server.
        && !path.contains('%')
        && !path
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '#');
    if !plain || !METHODS.contains(&method) {
        return false;
    }
    match path {
        "/v1/me" => method == "GET",
        // The profile picture can be set (PUT) and removed (DELETE).
        "/v1/me/avatar" => method == "PUT" || method == "DELETE",
        // The company directory of a self-hosted installation (its administrators only; the server decides).
        "/v1/admin/ldap" => method == "GET" || method == "PUT" || method == "DELETE",
        "/v1/admin/ldap/test" | "/v1/admin/ldap/sync" => method == "POST",
        _ => org_segments(path).is_some_and(|s| org_route(method, &s).is_some()),
    }
}

/// The text of the native question the app asks the person before sending this, if any: destructive organization
/// changes must not depend on the web view alone (script running in it could otherwise send them).
pub fn confirmation(method: &str, path: &str) -> Option<&'static str> {
    const LDAP: &str = "Change or remove the company directory? People may no longer be able to sign in with it.\n\nDas Firmenverzeichnis ändern oder entfernen? Personen können sich eventuell nicht mehr damit anmelden.";
    let bare = path.split('?').next().unwrap_or("");
    if bare == "/v1/admin/ldap" && (method == "PUT" || method == "DELETE") {
        return Some(LDAP);
    }
    org_route(method, &org_segments(path)?).flatten()
}

async fn confirm(app: &AppHandle, text: &str) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(text)
        .title("Zeima")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Confirm / Bestätigen".into(),
            "Cancel / Abbrechen".into(),
        ))
        .show(move |yes| {
            let _ = tx.send(yes);
        });
    rx.await.unwrap_or(false)
}

/// The answer: status and body text.
pub(crate) fn call(
    method: &str,
    url: &str,
    token: &str,
    body: Option<&str>,
) -> Result<(u16, String), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(HTTP_TIMEOUT))
        .build()
        .into();
    let auth = format!("Bearer {token}");
    let json = |b: Option<&str>| b.unwrap_or("{}").to_string();
    let mut response = match method {
        "GET" => agent.get(url).header("Authorization", &auth).call(),
        "DELETE" => agent.delete(url).header("Authorization", &auth).call(),
        "POST" => agent
            .post(url)
            .header("Authorization", &auth)
            .header("Content-Type", "application/json")
            .send(json(body)),
        "PUT" => agent
            .put(url)
            .header("Authorization", &auth)
            .header("Content-Type", "application/json")
            .send(json(body)),
        _ => agent
            .patch(url)
            .header("Authorization", &auth)
            .header("Content-Type", "application/json")
            .send(json(body)),
    }
    .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
    Ok((status, text))
}

/// Sends a request to the signed-in server's organization API. Returns the
/// JSON answer as text (`None` for "no content"); the server's problem
/// details become the error, with its `code`.
#[tauri::command]
#[specta::specta]
pub async fn org_request(
    app: AppHandle,
    method: String,
    path: String,
    body: Option<String>,
) -> CmdResult<Option<String>> {
    if !allowed(&method, &path) {
        return Err(err("forbidden", "this request is not allowed"));
    }
    if let Some(text) = confirmation(&method, &path) {
        if !confirm(&app, text).await {
            return Err(err("cancelled", "not confirmed"));
        }
    }
    let Some((server, token)) = account::current_token(&app).await.map_err(|e| err("server", e))? else {
        return Err(err("signed_out", "sign in to manage an organization"));
    };
    let (discovery, _) = tauri::async_runtime::spawn_blocking(move || discover(&server))
        .await
        .map_err(|e| err("io", e))?
        .map_err(|e| err("server", e))?;
    let url = format!("{}{path}", discovery.api_url.trim_end_matches('/'));
    let changes = method != "GET";
    let m = method.clone();
    let (status, text) =
        tauri::async_runtime::spawn_blocking(move || call(&m, &url, &token, body.as_deref()))
            .await
            .map_err(|e| err("io", e))?
            .map_err(|e| err("server", format!("could not reach the server: {e}")))?;
    if changes {
        // Roles and organizations are in the token's groups: fetch a new token next time.
        app.state::<Addons>().tokens.forget(ACCOUNT_ID, None);
    }
    match status {
        200..=299 if text.trim().is_empty() || status == 204 => Ok(None),
        200..=299 => Ok(Some(text)),
        _ => {
            let problem = serde_json::from_str::<serde_json::Value>(&text).ok();
            let field = |k: &str| {
                problem
                    .as_ref()
                    .and_then(|v| v.get(k))
                    .and_then(|v| v.as_str())
                    .map(String::from)
            };
            Err(err(
                &field("code").unwrap_or_else(|| "server".into()),
                field("detail").unwrap_or_else(|| format!("the server answered {status}")),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{allowed, confirmation};

    #[test]
    fn route_table_methods_and_confirmations() {
        // (method, path, allowed, confirmed by the app)
        let table = [
            ("GET", "/v1/orgs", true, false),
            ("POST", "/v1/orgs", true, false),
            ("DELETE", "/v1/orgs", false, false),
            ("PUT", "/v1/orgs", false, false),
            ("PATCH", "/v1/orgs", false, false),
            ("GET", "/v1/orgs/o", true, false),
            ("DELETE", "/v1/orgs/o", true, true),
            ("PATCH", "/v1/orgs/o", false, false),
            ("GET", "/v1/orgs/o/members?limit=5", true, false),
            ("POST", "/v1/orgs/o/members", false, false),
            ("PATCH", "/v1/orgs/o/members/u", true, true),
            ("DELETE", "/v1/orgs/o/members/u", true, true),
            ("PUT", "/v1/orgs/o/members/u/job", true, false),
            ("DELETE", "/v1/orgs/o/audit", false, false),
            ("GET", "/v1/orgs/o/audit?limit=1", true, false),
            ("PUT", "/v1/orgs/o/addons", true, true),
            ("GET", "/v1/orgs/o/addons", true, false),
            ("DELETE", "/v1/orgs/o/addons", false, false),
            ("PATCH", "/v1/orgs/o/roles/r", true, false),
            ("DELETE", "/v1/orgs/o/roles/r", true, true),
            ("PUT", "/v1/orgs/o/sso", true, true),
            ("DELETE", "/v1/orgs/o/sso", true, true),
            ("DELETE", "/v1/orgs/o/scim/tokens/t", true, true),
            ("POST", "/v1/orgs/o/scim/tokens", true, false),
            ("PUT", "/v1/orgs/o/groups/g/members/u", true, false),
            ("DELETE", "/v1/orgs/o/jobs/j", true, false),
            ("GET", "/v1/orgs/o/unknown", false, false),
            ("PUT", "/v1/admin/ldap", true, true),
            ("DELETE", "/v1/admin/ldap", true, true),
            ("GET", "/v1/admin/ldap", true, false),
            ("POST", "/v1/admin/ldap/test", true, false),
        ];
        for (m, p, ok, ask) in table {
            assert_eq!(allowed(m, p), ok, "allowed {m} {p}");
            if ok {
                assert_eq!(confirmation(m, p).is_some(), ask, "confirmation {m} {p}");
            }
        }
    }

    #[test]
    fn non_canonical_organization_paths_are_refused() {
        assert!(!allowed("DELETE", "/v1/orgs/./x"));
        assert!(!allowed("DELETE", "/v1/orgs/x/"));
        assert!(!allowed("DELETE", "/v1/orgs/x/./"));
        assert!(!allowed("DELETE", "/v1/orgs/x/."));
        assert!(!allowed("GET", "/v1/orgs/./x"));
        assert!(confirmation("DELETE", "/v1/orgs/x/").is_none());
        assert!(allowed("DELETE", "/v1/orgs/x"));
        assert!(allowed("GET", "/v1/orgs/x/members?limit=5"));
        assert!(allowed("GET", "/v1/orgs"));
    }

    #[test]
    fn only_organization_routes_are_allowed() {
        assert!(allowed("GET", "/v1/me"));
        assert!(allowed("POST", "/v1/orgs"));
        assert!(allowed("GET", "/v1/orgs/0b2f/members"));
        assert!(allowed("PATCH", "/v1/orgs/0b2f/members/8a1c"));
        assert!(allowed("GET", "/v1/orgs/0b2f/audit?before=5&limit=10"));
        assert!(!allowed("POST", "/v1/me"));
        assert!(!allowed("GET", "/v1/entitlement"));
        assert!(allowed("PUT", "/v1/me/avatar"));
        assert!(allowed("DELETE", "/v1/me/avatar"));
        assert!(!allowed("GET", "/v1/me/avatar"));
        assert!(!allowed("PUT", "/v1/me"));
        assert!(!allowed("PUT", "/v1/me/other"));
        assert!(!allowed("GET", "/v1/orgs/"));
        assert!(!allowed("GET", "/v1/orgs/../license"));
        assert!(!allowed("GET", "/v1/orgs//x"));
        assert!(!allowed("GET", "/v1/orgsX"));
        assert!(!allowed("GET", "/v1/orgs/a b"));
        assert!(!allowed("GET", "http://evil.example/v1/orgs/a"));
        assert!(!allowed("TRACE", "/v1/orgs/a"));
        assert!(!allowed("GET", "/v1/orgs/ä"));
        // Percent-encoded dots or slashes could be decoded by the server into a path the checks above refuse.
        assert!(!allowed("GET", "/v1/orgs/%2e%2e/license"));
        assert!(!allowed("GET", "/v1/orgs/a%2fb"));
        assert!(!allowed("GET", "/v1/orgs/a%2Fb/members"));
        // The company directory: exactly its routes, with the methods they have.
        assert!(allowed("GET", "/v1/admin/ldap"));
        assert!(allowed("PUT", "/v1/admin/ldap"));
        assert!(allowed("DELETE", "/v1/admin/ldap"));
        assert!(allowed("POST", "/v1/admin/ldap/test"));
        assert!(allowed("POST", "/v1/admin/ldap/sync"));
        assert!(!allowed("POST", "/v1/admin/ldap"));
        assert!(!allowed("GET", "/v1/admin/ldap/sync"));
        assert!(!allowed("GET", "/v1/admin/license"));
        assert!(!allowed("GET", "/v1/admin/ldap/../x"));
    }
}
