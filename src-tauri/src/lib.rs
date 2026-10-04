use std::sync::Mutex;

use tauri::{Manager, WindowEvent};

use tauri::menu::{Menu, MenuItem};

use tauri::tray::{TrayIcon, TrayIconBuilder};

use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use tauri_plugin_window_state::{
    StateFlags,
    //WindowExt,
};

struct TrayState {
    tray: Mutex<Option<TrayIcon>>,
}

fn create_tray_menu(
    app: &tauri::AppHandle,
    window_visible: bool,
) -> tauri::Result<Menu<tauri::Wry>> {
    let show_or_hide = if window_visible {
        MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?
    } else {
        MenuItem::with_id(app, "show", "Show", true, None::<&str>)?
    };

    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    Menu::with_items(app, &[&show_or_hide, &quit])
}

fn update_tray_menu(app: &tauri::AppHandle, visible: bool) {
    let state = app.state::<TrayState>();

    let guard = state.tray.lock().unwrap();

    if let Some(tray) = guard.as_ref() {
        if let Ok(menu) = create_tray_menu(app, visible) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_sql::Builder::new().build())
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(StateFlags::all())
                .build(),
        )
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let window = app.get_webview_window("main").expect("main window missing");

            //window
            //    .restore_state(StateFlags::all())
            //    .ok();

            let _ = window.show();
            let _ = window.set_focus();

            // -------------------------
            // Tray
            // -------------------------

            let menu = create_tray_menu(app.handle(), true)?;

            let tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .on_menu_event(|app, event| {
                    let window = app.get_webview_window("main").unwrap();

                    match event.id().as_ref() {
                        "show" => {
                            let _ = window.show();
                            let _ = window.set_focus();

                            update_tray_menu(app, true);
                        }

                        "hide" => {
                            let _ = window.hide();

                            update_tray_menu(app, false);
                        }

                        "quit" => {
                            app.exit(0);
                        }

                        _ => {}
                    }
                })
                .build(app)?;

            app.manage(TrayState {
                tray: Mutex::new(Some(tray)),
            });

            // -------------------------
            // Global shortcut
            // -------------------------

            let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space);

            let app_handle = app.handle().clone();

            app.global_shortcut()
                .on_shortcut(shortcut, move |_app, _shortcut, event| {
                    if event.state != ShortcutState::Pressed {
                        return;
                    }

                    let window = app_handle.get_webview_window("main").unwrap();

                    let visible = window.is_visible().unwrap_or(false);

                    if visible {
                        let _ = window.hide();

                        update_tray_menu(&app_handle, false);
                    } else {
                        let _ = window.show();
                        let _ = window.set_focus();

                        update_tray_menu(&app_handle, true);
                    }
                })?;

            // -------------------------
            // Close => hide
            // -------------------------

            let window_clone = window.clone();
            let app_handle = app.handle().clone();

            window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();

                    let _ = window_clone.hide();

                    update_tray_menu(&app_handle, false);
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri");
}
