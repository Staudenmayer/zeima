//! Domain events: what happened to the data, for addons and other
//! listeners. Repositories queue them on the write transaction; they are
//! published only after it commits, so listeners never see rolled-back
//! changes.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::model::TimeEntry;
use crate::worktime::WorkDay;

/// Serialized as `{ "type": "entry.started", "entry": … }`. The `type`
/// strings are the addon hook names (docs/addons.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "type")]
pub enum DomainEvent {
    /// The timer started an entry (also sent as `entry.created`).
    #[serde(rename = "entry.started")]
    EntryStarted { entry: TimeEntry },
    /// The running entry was stopped or paused (also sent as `entry.updated`).
    #[serde(rename = "entry.stopped")]
    EntryStopped { entry: TimeEntry },
    #[serde(rename = "entry.created")]
    EntryCreated { entry: TimeEntry },
    #[serde(rename = "entry.updated")]
    EntryUpdated { entry: TimeEntry },
    #[serde(rename = "entry.deleted")]
    EntryDeleted {
        #[serde(rename = "entryId")]
        entry_id: String,
    },
    /// A day is over (sent once after midnight, also for missed days).
    #[serde(rename = "day.closed")]
    DayClosed { date: NaiveDate, day: WorkDay },
}

impl DomainEvent {
    /// The hook name, e.g. `entry.started`.
    pub fn hook(&self) -> &'static str {
        match self {
            Self::EntryStarted { .. } => "entry.started",
            Self::EntryStopped { .. } => "entry.stopped",
            Self::EntryCreated { .. } => "entry.created",
            Self::EntryUpdated { .. } => "entry.updated",
            Self::EntryDeleted { .. } => "entry.deleted",
            Self::DayClosed { .. } => "day.closed",
        }
    }
}
