//! Windows and macOS: idle time via `user-idle` (GetLastInputInfo / IOKit).
//! Lock state is not detected; a locked screen shows up as idle time.

use super::{ActivityProbe, Snapshot};

pub struct UserIdleProbe;

impl ActivityProbe for UserIdleProbe {
    fn snapshot(&mut self) -> Snapshot {
        Snapshot {
            idle_ms: user_idle::UserIdle::get_time()
                .ok()
                .map(|t| t.as_milliseconds().min(i64::MAX as u128) as i64),
            locked: None,
        }
    }

    fn describe(&self) -> String {
        "idle: user-idle; lock: via idle time".into()
    }
}
