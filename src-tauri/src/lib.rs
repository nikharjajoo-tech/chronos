pub mod analytics;
pub mod commands;
pub mod companion;
pub mod db;
pub mod engine;
pub mod eventkit;
pub mod insights;
pub mod report;
pub mod settings;
pub mod shell;
pub mod sysmac;
pub mod tasks;
pub mod timer;

use engine::Engine;
use shell::{AppState, CLOCK};
use std::sync::Mutex;
use tauri::{Manager, RunEvent};
use tauri_plugin_window_state::AppHandleExt;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(shell::window_state_flags())
                .build(),
        )
        .on_menu_event(shell::on_menu_event)
        .invoke_handler(tauri::generate_handler![
            commands::snapshot,
            commands::act,
            commands::tasks_list,
            commands::task_parse,
            commands::task_add,
            commands::task_update,
            commands::task_move,
            commands::task_set_done,
            commands::task_delete,
            commands::task_undo_delete,
            commands::today_stats,
            commands::period_stats,
            commands::insights,
            commands::respond_suggestion,
            commands::recent_projects,
            commands::log_distraction,
            commands::set_distraction_category,
            commands::rate_session,
            commands::quick_state,
            commands::quick_close,
            commands::helper_status,
            commands::install_dnd_shortcuts,
            commands::connect_status,
            commands::connect_request,
            commands::connect_lists,
            commands::connect_open_privacy,
            commands::email_status,
            commands::set_email_password,
            commands::send_test_email,
            commands::preview_email,
            commands::open_app_passwords,
            commands::get_settings,
            commands::set_settings,
            commands::toggle_clock,
            commands::open_panel,
            commands::plan_mode,
            commands::morning_state,
            commands::morning_commit,
            commands::morning_dismiss,
            commands::set_plan_mode,
            commands::set_panel_layout,
            commands::show_clock_menu,
            commands::set_clock_scale,
            commands::test_sound,
            commands::app_info,
            commands::reveal_data_dir,
        ])
        .setup(|app| {
            // A desk accessory: lives in the menu bar, not the Dock.
            // (Unless "Show in Dock" is on; applied with the other settings below.)
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db_path = dir.join("chronos.sqlite3");
            migrate_from_tock(&dir, &db_path);
            let db = db::Db::open(&db_path)?;
            // Keyed per app name: a history migrated from Tock has no saved
            // window position under the new identifier, so place it again.
            let first_run = db.kv_get::<bool>("placed_chronos").is_none();
            if first_run {
                db.kv_set("placed_chronos", &true)?;
            }
            let engine = Engine::load(db);
            let settings = engine.settings.clone();
            app.manage(AppState(Mutex::new(engine)));
            app.manage(companion::CompanionState(Mutex::new(Default::default())));

            let handle = app.handle().clone();
            shell::build_tray(&handle, &settings.hide_shortcut)?;
            if let Err(e) = shell::register_shortcuts(&handle, &settings, None) {
                eprintln!("{e}");
            }
            shell::apply_window_settings(&handle, &settings);
            shell::apply_app_settings(&handle, None, &settings);
            if first_run {
                shell::place_clock_first_run(&handle);
            }
            if let Some(w) = app.get_webview_window(CLOCK) {
                w.show()?;
            }
            companion::spawn_watchdog(handle);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Chronos")
        .run(|app, event| match event {
            // Closing the stats panel must not quit a menu-bar app.
            RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit(),
            // Opening the app again (Launchpad, Spotlight, Dock) brings the clock back.
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => shell::show_clock(app),
            RunEvent::Exit => {
                companion::dnd_off_now(app);
                let _ = app.save_window_state(shell::window_state_flags());
            }
            _ => {}
        });
}

/// The app was called Tock before; carry its history over on first launch.
fn migrate_from_tock(dir: &std::path::Path, db_path: &std::path::Path) {
    if db_path.exists() {
        return;
    }
    let Some(old_dir) = dir.parent().map(|p| p.join("com.tock.desk")) else { return };
    let old = old_dir.join("tock.sqlite3");
    if !old.exists() {
        return;
    }
    // Copy the WAL alongside so no recent writes are lost.
    for suffix in ["", "-wal", "-shm"] {
        let from = old_dir.join(format!("tock.sqlite3{suffix}"));
        if from.exists() {
            let _ = std::fs::copy(&from, dir.join(format!("chronos.sqlite3{suffix}")));
        }
    }
}
