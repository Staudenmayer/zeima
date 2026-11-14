use std::sync::{Arc, RwLock};

use zeima_core::model::WorkSettings;
use zeima_core::Core;

use crate::settings::AppSettings;

pub struct AppState {
    /// The personal workspace; settings, backups, sync and the addons live here.
    pub core: Arc<Core>,
    /// The organization workspace the person is working in, if not the
    /// personal one (see `workspaces.rs`).
    pub active: RwLock<Option<Arc<Core>>>,
    pub settings: RwLock<AppSettings>,
    /// Cached copy of the user's work settings (auto-pause config is read
    /// every few seconds by the activity watcher).
    pub work_settings: RwLock<WorkSettings>,
}

impl AppState {
    /// Where the person's data commands read and write: the active
    /// workspace, or the personal one.
    pub fn data(&self) -> Arc<Core> {
        self.active
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap_or_else(|| self.core.clone())
    }

    pub fn set_active(&self, core: Option<Arc<Core>>) {
        *self.active.write().unwrap_or_else(|e| e.into_inner()) = core;
    }

    pub fn settings(&self) -> AppSettings {
        self.settings.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn work_settings(&self) -> WorkSettings {
        self.work_settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_work_settings(&self, settings: WorkSettings) {
        *self.work_settings.write().unwrap_or_else(|e| e.into_inner()) = settings;
    }
}
