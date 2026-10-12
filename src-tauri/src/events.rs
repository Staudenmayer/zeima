//! Typed events from Rust to the webviews (exported to `src/bindings.ts`).

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;
use time_core::model::TimerState;

/// The timer started, stopped, paused or resumed.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct TimerChanged(pub TimerState);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DataKind {
    Activity,
    Project,
    Tag,
    Entry,
    Settings,
    CostCenter,
    /// Schedules, absences, holidays, balance corrections, work settings.
    Work,
    /// Auto-pause items or the morning check-in.
    Away,
}

/// Stored data changed; listeners reload what they show.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct DataChanged {
    pub kinds: Vec<DataKind>,
}

/// The quick-switch window was opened and should reset its input.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct QuickSwitchOpened;

/// Ask the main window to open a route (e.g. from the tray).
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct Navigate {
    pub path: String,
}

/// Pending prompts changed; the prompt window reloads.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct PromptsChanged;
