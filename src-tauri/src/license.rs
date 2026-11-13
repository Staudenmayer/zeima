//! The user's plan on this device (SPEC §2). The server hands out a signed
//! entitlement token (`GET /v1/entitlement`); this module keeps the latest
//! one, checks its signature against the keys built into the app and answers
//! `has_feature`. Checks work offline: a token keeps working for its grace
//! period after the paid period ends, and then the device is Free. Free never
//! locks local data.
//!
//! Trusted keys: the vendor keys given at build time in
//! `ZEIMA_ENTITLEMENT_KEYS` (raw Ed25519 public keys, base64url,
//! comma-separated), plus the development key in debug builds.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use zeima_core::entitlement::{self, Entitlement, Status, VerifyingKey};

use crate::commands::CmdResult;
use crate::state::AppState;

/// `local_meta` key of the latest token.
const TOKEN_KEY: &str = "entitlement_token";

/// The development key (public on purpose; see its file).
const DEV_KEY_FILE: &str = include_str!("../../packages/shared-fixtures/entitlement/dev-key.json");

#[derive(Deserialize)]
struct DevKey {
    #[serde(rename = "publicKey")]
    public_key: String,
}

pub fn trusted_keys() -> Vec<VerifyingKey> {
    let mut raw: Vec<String> = option_env!("ZEIMA_ENTITLEMENT_KEYS")
        .unwrap_or("")
        .split(',')
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .collect();
    if cfg!(debug_assertions) {
        if let Ok(dev) = serde_json::from_str::<DevKey>(DEV_KEY_FILE) {
            raw.push(dev.public_key);
        }
    }
    raw.iter().filter_map(|k| entitlement::parse_key(k)).collect()
}

/// What this device is entitled to now, from the stored token. `None` is Free.
pub async fn current(app: &AppHandle) -> Option<Entitlement> {
    let state = app.state::<AppState>();
    let token = state.core.get_meta(TOKEN_KEY).await.ok().flatten()?;
    let now = state.core.now_ms() / 1000;
    entitlement::verify(&token, &trusted_keys(), now)
        .ok()
        .filter(|e| e.status != Status::Expired)
}

/// Keeps a token the server sent, if it verifies.
pub async fn store(app: &AppHandle, token: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let now = state.core.now_ms() / 1000;
    entitlement::verify(token, &trusted_keys(), now).map_err(|e| format!("the server's entitlement: {e}"))?;
    state
        .core
        .set_meta(TOKEN_KEY, token)
        .await
        .map_err(|e| e.to_string())
}

/// Forgets the token (signing out, or the server stopped issuing one).
pub async fn clear(app: &AppHandle) {
    let _ = app.state::<AppState>().core.set_meta(TOKEN_KEY, "").await;
}

pub async fn has_feature(app: &AppHandle, feature: &str) -> bool {
    entitlement::has_feature(current(app).await.as_ref(), feature)
}

#[derive(Debug, Clone, Copy, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Valid,
    Grace,
    /// No token, or it lapsed.
    Free,
}

#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanInfo {
    pub plan: String,
    pub status: PlanStatus,
    pub features: Vec<String>,
    /// End of the paid period, unix ms.
    #[specta(type = Option<specta_typescript::Number>)]
    pub expires_at: Option<i64>,
}

#[tauri::command]
#[specta::specta]
pub async fn plan_status(app: AppHandle) -> CmdResult<PlanInfo> {
    Ok(match current(&app).await {
        Some(e) => PlanInfo {
            plan: e.plan.clone(),
            status: if e.status == Status::Grace {
                PlanStatus::Grace
            } else {
                PlanStatus::Valid
            },
            features: e.features.clone(),
            expires_at: Some(e.claims.exp * 1000),
        },
        None => PlanInfo {
            plan: "free".into(),
            status: PlanStatus::Free,
            features: Vec::new(),
            expires_at: None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_builds_trust_the_development_key() {
        assert!(!trusted_keys().is_empty());
    }
}
