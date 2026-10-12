use std::sync::{Arc, RwLock};

use zeima_core::model::WorkSettings;
use zeima_core::Core;

use crate::settings::AppSettings;

pub struct AppState {
    pub core: Arc<Core>,
    pub settings: RwLock<AppSettings>,
    /// Cached copy of the user's work settings (auto-pause config is read
    /// every few seconds by the activity watcher).
    pub work_settings: RwLock<WorkSettings>,
}

impl AppState {
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
