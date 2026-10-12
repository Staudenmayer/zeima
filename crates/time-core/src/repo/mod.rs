//! Repositories. Each is an `impl Core` block; writes go through
//! [`crate::db::WriteTx`] so they are stamped and change-logged.

mod addons;
mod budget;
mod cost_centers;
mod data;
mod entries;
mod projects;
mod tags;
mod work;

pub use addons::AddonRecord;
pub use budget::BudgetStatus;
pub(crate) use work::invalidate_entry_days;
pub use work::{default_schedule, Balance, HolidayView, ProfileSummary, VacationSummary, WorkScheduleInput};

pub(crate) use cost_centers::{check_cost_center_ref, resolve_cost_center};
pub(crate) use entries::{fetch_entry, fetch_running, insert_entry, new_entry, update_entry_row};
pub(crate) use projects::{ensure_trackable, fetch_project, soft_delete};
pub(crate) use tags::validate_entry_tags;

use crate::Error;

/// Turns a unique-index violation into a validation error on `field`.
pub(crate) fn unique_violation(err: sqlx::Error, field: &str, message: &str) -> Error {
    match &err {
        sqlx::Error::Database(db) if db.is_unique_violation() => Error::validation(field, message),
        _ => Error::Db(err),
    }
}

pub(crate) fn not_found(entity: &'static str, id: &str) -> Error {
    Error::NotFound {
        entity,
        id: id.to_string(),
    }
}
