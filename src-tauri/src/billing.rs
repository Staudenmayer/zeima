//! Buying and managing a plan (cloud servers only; SPEC §2). The server
//! builds the checkout link and tells the app about the subscription; the
//! purchase itself happens in the browser, and the new plan arrives with the
//! next sync (`license.rs`).

use std::time::Duration;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::account::{self, discover};
use crate::commands::{CmdError, CmdResult};

const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

fn err(code: &str, message: impl ToString) -> CmdError {
    CmdError::other(code, message)
}

/// One request to the API: `Ok(None)` when the server does not have the
/// endpoint (self-hosted servers have no billing).
fn call(
    method: &str,
    url: &str,
    token: &str,
    body: Option<&serde_json::Value>,
) -> Result<Option<serde_json::Value>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(HTTP_TIMEOUT))
        .build()
        .into();
    let auth = format!("Bearer {token}");
    let mut response = if method == "POST" {
        agent
            .post(url)
            .header("Authorization", &auth)
            .header("Content-Type", "application/json")
            .send(body.map(ToString::to_string).unwrap_or_default())
    } else {
        agent.get(url).header("Authorization", &auth).call()
    }
    .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let text = response.body_mut().read_to_string().map_err(|e| e.to_string())?;
    match status {
        404 => Ok(None),
        200..=299 => serde_json::from_str(&text).map(Some).map_err(|e| e.to_string()),
        _ => {
            let detail = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v.get("detail").and_then(|d| d.as_str()).map(String::from));
            Err(detail.unwrap_or_else(|| format!("the server answered {status}")))
        }
    }
}

/// `api` base URL and a token, or `None` when signed out.
async fn api(app: &AppHandle) -> Result<Option<(String, String)>, String> {
    let Some((server, token)) = account::current_token(app).await? else {
        return Ok(None);
    };
    let (discovery, _) = tauri::async_runtime::spawn_blocking(move || discover(&server))
        .await
        .map_err(|e| e.to_string())??;
    Ok(Some((discovery.api_url.trim_end_matches('/').to_string(), token)))
}

async fn request(
    app: &AppHandle,
    method: &'static str,
    path: &'static str,
    body: Option<serde_json::Value>,
) -> Result<Option<serde_json::Value>, String> {
    let Some((api, token)) = api(app).await? else {
        return Ok(None);
    };
    let url = format!("{api}{path}");
    tauri::async_runtime::spawn_blocking(move || call(method, &url, &token, body.as_ref()))
        .await
        .map_err(|e| e.to_string())?
}

#[derive(Debug, Deserialize, Serialize, Type)]
pub struct PlanOffer {
    pub key: String,
    pub plan: String,
}

/// What can be bought here. Empty on servers without billing and when signed out.
#[tauri::command]
#[specta::specta]
pub async fn billing_plans(app: AppHandle) -> CmdResult<Vec<PlanOffer>> {
    let answer = request(&app, "GET", "/v1/billing/plans", None)
        .await
        .map_err(|e| err("server", e))?;
    Ok(answer
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

/// Opens the purchase page for a plan in the browser.
#[tauri::command]
#[specta::specta]
pub async fn billing_checkout(app: AppHandle, key: String) -> CmdResult<()> {
    let answer = request(
        &app,
        "POST",
        "/v1/billing/checkout",
        Some(serde_json::json!({ "key": key })),
    )
    .await
    .map_err(|e| err("server", e))?
    .ok_or_else(|| err("server", "this server has no purchases"))?;
    let url = answer
        .get("url")
        .and_then(|u| u.as_str())
        .ok_or_else(|| err("server", "the server sent no checkout link"))?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| err("browser", format!("could not open the browser: {e}")))
}

#[derive(Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub plan: String,
    pub status: String,
    #[specta(type = Option<specta_typescript::Number>)]
    pub valid_until: Option<i64>,
    pub portal_url: Option<String>,
}

/// The caller's subscription, if they have bought one.
#[tauri::command]
#[specta::specta]
pub async fn billing_subscription(app: AppHandle) -> CmdResult<Option<Subscription>> {
    let answer = request(&app, "GET", "/v1/billing", None)
        .await
        .map_err(|e| err("server", e))?;
    Ok(answer.and_then(|v| serde_json::from_value(v).ok()))
}

/// Opens the page where the customer manages or cancels the subscription.
#[tauri::command]
#[specta::specta]
pub async fn billing_open_portal(app: AppHandle) -> CmdResult<()> {
    let subscription = billing_subscription(app.clone()).await?;
    let url = subscription
        .and_then(|s| s.portal_url)
        .ok_or_else(|| err("server", "there is no subscription to manage"))?;
    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|e| err("browser", format!("could not open the browser: {e}")))
}
