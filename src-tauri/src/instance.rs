//! Running several copies of the app on one machine, for testing sync between
//! two "devices". Set `ZEIMA_DATA_DIR` to a folder and the app uses it instead
//! of the usual data folder, which makes it a separate device:
//!
//! ```sh
//! bun run tauri dev                                    # the normal instance
//! ZEIMA_DATA_DIR=/tmp/zeima-b ./target/debug/zeima     # a second one (debug builds load the dev server)
//! ```
//!
//! Such an instance
//!  - has its own database, backups and addons (and so its own device id),
//!  - is not stopped by the single-instance check, and does not stop others,
//!  - keeps its sign-in in its own keychain entries, so it can use another
//!    account or the same one without overwriting the other's tokens,
//!  - keeps its own window positions,
//!  - never changes the operating system's autostart setting,
//!  - shows its folder name in the window title.

use std::path::PathBuf;
use std::sync::OnceLock;

use tauri::{AppHandle, Manager};

/// The folder from `ZEIMA_DATA_DIR`, if set and not empty.
pub fn data_dir_override() -> Option<&'static PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        std::env::var_os("ZEIMA_DATA_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    })
    .as_ref()
}

/// True for a test instance started with `ZEIMA_DATA_DIR`.
pub fn is_secondary() -> bool {
    data_dir_override().is_some()
}

/// The folder for the database, backups and addons.
pub fn data_dir(app: &AppHandle) -> tauri::Result<PathBuf> {
    match data_dir_override() {
        Some(dir) => Ok(dir.clone()),
        None => app.path().app_data_dir(),
    }
}

/// Short name of a test instance (its folder name), for titles and file names.
pub fn label() -> Option<String> {
    data_dir_override().map(|dir| {
        dir.file_name()
            .map_or_else(|| "test".to_string(), |n| n.to_string_lossy().into_owned())
    })
}

/// A short stable tag derived from the folder, for names that must differ
/// between instances (keychain service, window-state file).
pub fn tag() -> Option<String> {
    data_dir_override().map(|dir| {
        // FNV-1a: stable across runs and platforms (unlike DefaultHasher).
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in dir.to_string_lossy().as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        format!("{:08x}", hash >> 32)
    })
}

/// Keychain "service" the app files its entries under.
pub fn keychain_service(base: &str) -> String {
    match tag() {
        Some(tag) => format!("{base}.{tag}"),
        None => base.to_string(),
    }
}

/// File name of the window-position store.
pub fn window_state_file() -> String {
    match tag() {
        Some(tag) => format!(".window-state-{tag}.json"),
        None => ".window-state.json".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_normal_instance_keeps_the_usual_names() {
        // The tests run without ZEIMA_DATA_DIR.
        if std::env::var_os("ZEIMA_DATA_DIR").is_some() {
            return;
        }
        assert!(!is_secondary());
        assert_eq!(keychain_service("com.zeima.zeima"), "com.zeima.zeima");
        assert_eq!(window_state_file(), ".window-state.json");
        assert_eq!(label(), None);
    }
}
