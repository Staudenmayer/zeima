//! Which workspace the person is working in: their personal one or an
//! organization's shared one (SPEC §5.9). The data commands follow
//! `AppState::data()`; settings, backups, sync and addons stay personal.
//! Only one timer runs at a time, across workspaces.

use std::sync::Arc;

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager, State};
use zeima_core::Core;

use crate::commands::{changed, CmdError, CmdResult};
use crate::events::DataKind;
use crate::state::AppState;

/// `local_meta`: the Keycloak account the shared workspaces are used as.
pub const ACCOUNT_USER_KEY: &str = "account_user_id";
/// `local_meta`: the workspace the person last worked in.
const ACTIVE_KEY: &str = "active_workspace";

#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceView {
    pub id: String,
    /// `personal` or `org`.
    pub kind: String,
    pub name: String,
    pub org_id: Option<String>,
    pub active: bool,
}

/// A handle on `workspace_id`: the personal core itself, or a scoped one for
/// an organization's workspace.
async fn open(core: &Arc<Core>, workspace_id: &str) -> Result<Option<Arc<Core>>, CmdError> {
    if workspace_id == core.ctx().workspace_id {
        return Ok(Some(core.clone()));
    }
    let known = core
        .workspaces()
        .await?
        .into_iter()
        .any(|w| w.id == workspace_id && w.kind == "org");
    if !known {
        return Ok(None);
    }
    let Some(user) = core.get_meta(ACCOUNT_USER_KEY).await? else {
        return Err(CmdError::other(
            "signed_out",
            "sign in and sync once to work in an organization",
        ));
    };
    Ok(Some(Arc::new(core.scoped(workspace_id, &user))))
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_list(state: State<'_, AppState>) -> CmdResult<Vec<WorkspaceView>> {
    let active = state.data().ctx().workspace_id.clone();
    Ok(state
        .core
        .workspaces()
        .await?
        .into_iter()
        .map(|w| WorkspaceView {
            active: w.id == active,
            id: w.id,
            kind: w.kind,
            name: w.name,
            org_id: w.org_id,
        })
        .collect())
}

/// Every workspace the person can track time in, each as a handle.
async fn all(state: &AppState) -> Vec<Arc<Core>> {
    let mut cores = vec![state.core.clone()];
    if let Ok(Some(user)) = state.core.get_meta(ACCOUNT_USER_KEY).await {
        if let Ok(list) = state.core.workspaces().await {
            cores.extend(
                list.into_iter()
                    .filter(|w| w.kind == "org")
                    .map(|w| Arc::new(state.core.scoped(&w.id, &user))),
            );
        }
    }
    cores
}

/// Stops the timer of every workspace but the active one: one timer at a time.
pub async fn stop_other_timers(state: &AppState) -> zeima_core::Result<bool> {
    let current = state.data().ctx().workspace_id.clone();
    let mut stopped = false;
    for core in all(state).await {
        if core.ctx().workspace_id == current {
            continue;
        }
        if !matches!(core.timer_state().await?, zeima_core::model::TimerState::Idle) {
            core.stop_timer().await?;
            stopped = true;
        }
    }
    Ok(stopped)
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_switch(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let Some(core) = open(&state.core, &id).await? else {
        return Err(CmdError::other(
            "not_found",
            "this workspace is not on this device",
        ));
    };
    let personal = core.ctx().workspace_id == state.core.ctx().workspace_id;
    state.set_active(if personal { None } else { Some(core) });
    state
        .core
        .set_meta(ACTIVE_KEY, if personal { "" } else { &id })
        .await?;
    // Everything on screen belongs to the other workspace now.
    changed(
        &app,
        &[
            DataKind::Activity,
            DataKind::Project,
            DataKind::Tag,
            DataKind::Entry,
            DataKind::CostCenter,
            DataKind::Work,
        ],
    )
    .await;
    Ok(())
}

/// Goes back to the workspace the person last worked in.
pub fn restore(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let Ok(Some(id)) = state.core.get_meta(ACTIVE_KEY).await else {
            return;
        };
        if id.is_empty() {
            return;
        }
        if let Ok(Some(core)) = open(&state.core, &id).await {
            state.set_active(Some(core));
        }
    });
}
