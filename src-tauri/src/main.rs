// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // WebKitGTK's accelerated compositing makes hover effects, dropdowns
    // and animations flicker on some Wayland setups (seen on KDE, AMD,
    // mixed 60/165 Hz monitors). Without it the UI is drawn directly,
    // which is fast enough for this app. It can only be chosen before any
    // view loads (switching a loaded view leaves it black), so it applies
    // to all windows; without compositing WebKit cannot draw transparent
    // windows, so the small windows are opaque on Linux (see windows.rs).
    // Set the variable to 0 to keep compositing on.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_COMPOSITING_MODE").is_none() {
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    }
    time_app_lib::run()
}
