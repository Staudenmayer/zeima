//! Main window and quick-switch window handling.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tauri_specta::Event;

use crate::events::{Navigate, QuickSwitchOpened};
use crate::tray;

pub const MAIN: &str = "main";
pub const QUICKSWITCH: &str = "quickswitch";

fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(MAIN)
}

pub fn main_visible(app: &AppHandle) -> bool {
    main_window(app)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

pub async fn show_main(app: &AppHandle, route: Option<&str>) {
    if let Some(window) = main_window(app) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        if let Some(path) = route {
            let _ = Navigate { path: path.into() }.emit_to(app, MAIN);
        }
    }
    tray::refresh(app).await;
}

pub async fn hide_main(app: &AppHandle) {
    if let Some(window) = main_window(app) {
        let _ = window.hide();
    }
    tray::refresh(app).await;
}

pub async fn toggle_main(app: &AppHandle) {
    if main_visible(app) {
        hide_main(app).await;
    } else {
        show_main(app, None).await;
    }
}

/// Closing the main window hides it; the app keeps running in the tray.
pub fn hide_main_on_close(app: &AppHandle) {
    let Some(window) = main_window(app) else { return };
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let handle = handle.clone();
            tauri::async_runtime::spawn(async move { hide_main(&handle).await });
        }
    });
}

/// Shows the quick-switch palette, creating it on first use. It hides
/// itself again when it loses focus.
pub fn show_quickswitch(app: &AppHandle) {
    let window = match app.get_webview_window(QUICKSWITCH) {
        Some(window) => window,
        None => match create_quickswitch(app) {
            Ok(window) => window,
            Err(err) => {
                eprintln!("failed to create quick switch window: {err}");
                return;
            }
        },
    };
    let _ = window.center();
    let _ = window.show();
    let _ = window.set_focus();
    let _ = QuickSwitchOpened.emit_to(app, QUICKSWITCH);
}

/// The quick-switch shortcut: opens the palette, or closes it when it is
/// already open.
pub fn toggle_quickswitch(app: &AppHandle) {
    let open = app
        .get_webview_window(QUICKSWITCH)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    if open {
        hide_quickswitch(app);
    } else {
        show_quickswitch(app);
    }
}

pub fn hide_quickswitch(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(QUICKSWITCH) {
        let _ = window.hide();
    }
}

fn create_quickswitch(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let builder = WebviewWindowBuilder::new(app, QUICKSWITCH, WebviewUrl::App("/quickswitch".into()))
        .title("Quick switch")
        .inner_size(560.0, 380.0)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .center();
    // Transparent so the page's rounded corners show. Not on Linux, where
    // WebKit runs without compositing (see main.rs) and cannot draw
    // transparent windows, nor on macOS (needs the private-API feature).
    #[cfg(target_os = "windows")]
    let builder = builder.transparent(true);
    let window = builder.build()?;
    let handle = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Focused(false) => {
            let _ = handle.hide();
        }
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let _ = handle.hide();
        }
        _ => {}
    });
    Ok(window)
}

pub const PROMPT: &str = "prompt";

/// Shows the prompt window (auto-pause decisions, morning check-in) in the
/// bottom-right corner without taking focus from the user's work.
pub fn show_prompt(app: &AppHandle) {
    let window = match app.get_webview_window(PROMPT) {
        Some(window) => window,
        None => match create_prompt(app) {
            Ok(window) => window,
            Err(err) => {
                eprintln!("failed to create prompt window: {err}");
                return;
            }
        },
    };
    let _ = window.show();
    let _ = crate::events::PromptsChanged.emit_to(app, PROMPT);
}

pub fn hide_prompt(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(PROMPT) {
        let _ = window.hide();
    }
}

fn create_prompt(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    const WIDTH: f64 = 400.0;
    const HEIGHT: f64 = 460.0;
    const MARGIN: f64 = 16.0;
    let mut builder = WebviewWindowBuilder::new(app, PROMPT, WebviewUrl::App("/prompt".into()))
        .title("Zeima")
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(false);
    #[cfg(target_os = "windows")]
    {
        builder = builder.transparent(true);
    }
    if let Some(monitor) = app.primary_monitor()? {
        let scale = monitor.scale_factor();
        let size = monitor.size().to_logical::<f64>(scale);
        let pos = monitor.position().to_logical::<f64>(scale);
        builder = builder.position(
            pos.x + size.width - WIDTH - MARGIN,
            pos.y + size.height - HEIGHT - MARGIN * 4.0,
        );
    }
    let window = builder.build()?;
    let handle = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = handle.hide();
        }
    });
    Ok(window)
}
