//! Shared test helpers.

use std::cell::RefCell;

use time_core::model::ActivityInput;
use time_core::Core;

thread_local! {
    // `#[tokio::test]` runs each test on its own single thread, so the
    // activity created by `with_activity` is visible to sync input helpers.
    static ACTIVITY: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Creates the default "Work" activity every new entry needs and returns
/// the core.
pub async fn with_activity(core: Core) -> Core {
    let activity = core
        .create_activity(ActivityInput {
            name: "Work".into(),
            color: "#3b82f6".into(),
            archived: false,
        })
        .await
        .unwrap();
    ACTIVITY.with(|a| *a.borrow_mut() = Some(activity.id));
    core
}

/// The activity created by [`with_activity`].
pub fn activity() -> Option<String> {
    ACTIVITY.with(|a| a.borrow().clone())
}
