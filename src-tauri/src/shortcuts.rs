//! Global shortcuts, configurable in settings.

use std::sync::Mutex;

use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Manager, Runtime, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::settings::AppSettings;
use crate::{commands, windows};

#[derive(Debug, Clone, Copy)]
enum Action {
    ToggleWindow,
    QuickSwitch,
    PauseResume,
}

#[derive(Default)]
struct Bindings(Mutex<Vec<(Shortcut, Action)>>);

pub fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            let Some(bindings) = app.try_state::<Bindings>() else {
                return;
            };
            let action = bindings
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|(s, _)| s == shortcut)
                .map(|(_, a)| *a);
            let app = app.clone();
            match action {
                Some(Action::ToggleWindow) => {
                    tauri::async_runtime::spawn(async move { windows::toggle_main(&app).await });
                }
                Some(Action::QuickSwitch) => windows::toggle_quickswitch(&app),
                Some(Action::PauseResume) => {
                    tauri::async_runtime::spawn(async move {
                        if let Err(err) = commands::toggle_pause(&app).await {
                            eprintln!("pause/resume failed: {err}");
                        }
                    });
                }
                None => {}
            }
        })
        .build()
}

fn parse(field: &str, value: &str) -> Result<Option<Shortcut>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    value
        .parse::<Shortcut>()
        .map(Some)
        .map_err(|e| format!("{field}: '{value}' is not a valid shortcut ({e})"))
}

/// Replaces all registered shortcuts with the ones from `settings`.
/// An empty shortcut disables that action.
pub fn apply<R: Runtime>(app: &AppHandle<R>, settings: &AppSettings) -> Result<(), String> {
    let wanted = [
        (
            parse("toggleWindow", &settings.shortcut_toggle_window)?,
            Action::ToggleWindow,
        ),
        (
            parse("quickSwitch", &settings.shortcut_quick_switch)?,
            Action::QuickSwitch,
        ),
        (
            parse("pauseResume", &settings.shortcut_pause_resume)?,
            Action::PauseResume,
        ),
    ];
    let bindings: Vec<(Shortcut, Action)> = wanted
        .into_iter()
        .filter_map(|(s, a)| s.map(|s| (s, a)))
        .collect();
    for (i, (a, _)) in bindings.iter().enumerate() {
        if bindings[..i].iter().any(|(b, _)| b == a) {
            return Err("the same shortcut is used twice".into());
        }
    }

    let gs = app.global_shortcut();
    gs.unregister_all().map_err(|e| e.to_string())?;
    for (shortcut, _) in &bindings {
        gs.register(*shortcut)
            .map_err(|e| format!("could not register {shortcut:?}: {e}"))?;
    }
    if app.try_state::<Bindings>().is_none() {
        app.manage(Bindings::default());
    }
    *app.state::<Bindings>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = bindings;
    Ok(())
}
