//! Domain core for Time.
//!
//! Pure modules (models, validation, clocks, summaries) compile without any
//! I/O so the crate can be built for WASM and reused by the server. SQLite
//! persistence and services live behind the `db` feature.

pub mod addon;
pub mod autopause;
pub mod clock;
pub mod error;
pub mod events;
pub mod export;
pub mod hlc;
pub mod ids;
pub mod jurisdiction;
pub mod locale;
pub mod model;
pub mod report;
pub mod summary;
pub mod worktime;

#[cfg(feature = "db")]
mod away;
#[cfg(feature = "db")]
pub mod db;
#[cfg(feature = "db")]
mod repo;
#[cfg(feature = "db")]
mod timer;

#[cfg(feature = "db")]
pub use away::{AwayItem, AwayResolution};

pub use error::{Error, Result};

#[cfg(feature = "db")]
pub use repo::{
    default_schedule, AddonRecord, Balance, BudgetStatus, HolidayView, ProfileSummary, VacationSummary,
    WorkScheduleInput,
};

#[cfg(feature = "db")]
pub use db::Core;
