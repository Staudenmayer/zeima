mod activity;
mod addons;
mod background;
mod backup;
mod budget;
mod commands;
mod commands_addons;
mod commands_report;
mod commands_work;
mod events;
mod os;
mod settings;
mod shortcuts;
mod state;
mod tray;
mod windows;

use std::sync::{Arc, RwLock};

use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_window_state::StateFlags;
use time_core::clock::SystemClock;
use time_core::Core;

use crate::state::AppState;

/// Passed by autostart so the app starts in the tray without a window.
const MINIMIZED_ARG: &str = "--minimized";

fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![
            commands::app_info,
            commands::project_list,
            commands::project_create,
            commands::project_update,
            commands::project_delete,
            commands::project_recent,
            commands::activity_list,
            commands::activity_create,
            commands::activity_update,
            commands::activity_delete,
            commands::activity_recent,
            commands::tag_list,
            commands::tag_create,
            commands::tag_update,
            commands::tag_delete,
            commands::entry_list,
            commands::entry_create,
            commands::entry_update,
            commands::entry_delete,
            commands::entry_bulk_update,
            commands::entry_bulk_delete,
            commands::entry_split,
            commands::entry_duplicate,
            commands::summaries,
            commands::timer_state,
            commands::timer_start,
            commands::timer_stop,
            commands::timer_pause,
            commands::timer_resume,
            commands::settings_get,
            commands::settings_set,
            commands::window_show_main,
            commands::quickswitch_show,
            commands::quickswitch_hide,
            commands::window_print,
            commands::timezone_list,
            commands_work::profile_list,
            commands_work::profile_suggest,
            commands_work::profile_summary,
            commands_work::jurisdiction_get,
            commands_work::jurisdiction_set,
            commands_work::work_settings_get,
            commands_work::work_settings_set,
            commands_work::schedule_list,
            commands_work::schedule_save,
            commands_work::schedule_delete,
            commands_work::absence_type_list,
            commands_work::absence_type_save,
            commands_work::absence_type_delete,
            commands_work::absence_list,
            commands_work::absence_save,
            commands_work::absence_delete,
            commands_work::vacation_summary,
            commands_work::holiday_list,
            commands_work::holiday_create,
            commands_work::holiday_delete,
            commands_work::adjustment_list,
            commands_work::adjustment_create,
            commands_work::adjustment_delete,
            commands_work::work_days,
            commands_work::balance,
            commands_work::prompt_state,
            commands_work::away_resolve,
            commands_work::morning_dismiss,
            commands_work::prompt_hide,
            commands_work::activity_source,
            commands_report::cost_center_list,
            commands_report::cost_center_save,
            commands_report::cost_center_delete,
            commands_report::cost_center_assignment_list,
            commands_report::cost_center_assignment_save,
            commands_report::cost_center_assignment_delete,
            commands_report::report,
            commands_report::budget_list,
            commands_report::report_export,
            commands_report::data_export_json,
            commands_report::backup_info,
            commands_report::backup_now,
            commands_report::backup_open_dir,
            commands_addons::addon_list,
            commands_addons::addon_pick,
            commands_addons::addon_install,
            commands_addons::addon_set_enabled,
            commands_addons::addon_save_settings,
            commands_addons::addon_uninstall,
            commands_addons::addon_export_formats,
            commands_addons::report_export_addon,
            commands_addons::addon_suggest,
        ])
        .events(tauri_specta::collect_events![
            events::TimerChanged,
            events::DataChanged,
            events::QuickSwitchOpened,
            events::Navigate,
            events::PromptsChanged,
        ])
}

const BINDINGS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/bindings.ts");

fn export_bindings(builder: &tauri_specta::Builder<tauri::Wry>) {
    builder
        .export(
            specta_typescript::Typescript::default().header("// @ts-nocheck\n/* eslint-disable */"),
            BINDINGS_PATH,
        )
        .expect("failed to export typescript bindings");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = specta_builder();
    #[cfg(debug_assertions)]
    export_bindings(&builder);

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { windows::show_main(&app, None).await });
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![MINIMIZED_ARG]),
        ))
        .plugin(
            tauri_plugin_window_state::Builder::new()
                // Visibility is decided at startup (autostart starts hidden).
                .with_state_flags(StateFlags::all() & !StateFlags::VISIBLE)
                .with_denylist(&[windows::QUICKSWITCH, windows::PROMPT])
                .build(),
        )
        .plugin(shortcuts::plugin())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);

            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let core =
                tauri::async_runtime::block_on(Core::open(&data_dir.join("time.db"), Arc::new(SystemClock)))?;
            let settings = tauri::async_runtime::block_on(settings::load(&core));
            // Before anything computes days: zone and week start from the settings.
            tauri::async_runtime::block_on(settings::apply_regional(&core, &settings));
            let work_settings = tauri::async_runtime::block_on(core.work_settings())?;
            if let Err(err) = shortcuts::apply(app.handle(), &settings) {
                eprintln!("shortcuts not registered: {err}");
            }
            app.manage(AppState {
                core: Arc::new(core),
                settings: RwLock::new(settings),
                work_settings: RwLock::new(work_settings),
            });
            app.manage(activity::Activity::default());
            app.manage(addons::Addons::default());
            addons::start(app.handle());

            tray::create(app.handle())?;
            windows::hide_main_on_close(app.handle());
            if !std::env::args().any(|a| a == MINIMIZED_ARG) {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move { windows::show_main(&handle, None).await });
            }
            background::spawn(app.handle().clone());
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move { activity::on_startup(&handle).await });
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move { backup::on_startup(&handle).await });
            activity::spawn(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    /// `cargo test` keeps `src/bindings.ts` up to date without starting the
    /// app; CI fails if the committed file differs.
    #[test]
    fn export_bindings() {
        super::export_bindings(&super::specta_builder());
    }
}
