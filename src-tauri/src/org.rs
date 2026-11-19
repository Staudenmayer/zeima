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

/// Whether the app may send this request: a known method and a plain path
/// below `/v1/orgs` (or exactly `/v1/me`), without tricks.
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
    let target = path == "/v1/me"
        || path == "/v1/me/avatar"
        || path == "/v1/admin/ldap"
        || path == "/v1/admin/ldap/test"
        || path == "/v1/admin/ldap/sync"
        || path == "/v1/orgs"
        || path
            .strip_prefix("/v1/orgs/")
            .is_some_and(|rest| !rest.is_empty());
    // `/v1/me` is read-only; the profile picture can be set (PUT) and removed (DELETE).
    let method_ok = match path {
        "/v1/me" => method == "GET",
        "/v1/me/avatar" => method == "PUT" || method == "DELETE",
        // The company directory of a self-hosted installation (its administrators only; the server decides).
        "/v1/admin/ldap" => method == "GET" || method == "PUT" || method == "DELETE",
        "/v1/admin/ldap/test" | "/v1/admin/ldap/sync" => method == "POST",
        _ => true,
    };
    METHODS.contains(&method) && plain && target && method_ok
}

/// Whether the app itself asks the person before sending this: deleting a whole organization cannot be undone, and
/// must not depend on the web view alone (script running in it could otherwise send it).
pub fn needs_confirmation(method: &str, path: &str) -> bool {
    let path = path.split('?').next().unwrap_or("");
    method == "DELETE" && path.strip_prefix("/v1/orgs/").is_some_and(|rest| !rest.is_empty() && !rest.contains('/'))
}

async fn confirm(app: &AppHandle, text: &str) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(text)
        .title("Zeima")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom("Delete / Löschen".into(), "Cancel / Abbrechen".into()))
        .show(move |yes| {
            let _ = tx.send(yes);
        });
    rx.await.unwrap_or(false)
}

/// The answer: status and body text.
pub(crate) fn call(method: &str, url: &str, token: &str, body: Option<&str>) -> Result<(u16, String), String> {
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
    if needs_confirmation(&method, &path)
        && !confirm(
            &app,
            "Delete this organization, its memberships and its shared data? This cannot be undone.\n\nDiese Organisation samt Mitgliedschaften und gemeinsamen Daten löschen? Das lässt sich nicht rückgängig machen.",
        )
        .await
    {
        return Err(err("cancelled", "not deleted"));
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
    use super::{allowed, needs_confirmation};

    #[test]
    fn only_deleting_an_organization_is_confirmed_by_the_app() {
        assert!(needs_confirmation("DELETE", "/v1/orgs/0b2f"));
        assert!(!needs_confirmation("DELETE", "/v1/orgs/0b2f/members/8a1c"));
        assert!(!needs_confirmation("GET", "/v1/orgs/0b2f"));
    }

    #[test]
    fn only_organization_routes_are_allowed() {
        assert!(allowed("GET", "/v1/me"));
        assert!(allowed("POST", "/v1/orgs"));
        assert!(allowed("GET", "/v1/orgs/0b2f/members"));
        assert!(allowed("PATCH", "/v1/orgs/0b2f/members/8a1c"));
        assert!(allowed("DELETE", "/v1/orgs/0b2f/audit?before=5&limit=10"));
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
