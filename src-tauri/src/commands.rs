//! IPC surface used by the web UI. Thin: each command forwards to the engine
//! or the shell and returns plain data.

use crate::db::DayStats;
use crate::engine::{now_ms, Action, Snapshot};
use crate::settings::Settings;
use crate::shell::{self, AppState, CLOCK};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

fn engine<'a>(state: &'a State<AppState>) -> std::sync::MutexGuard<'a, crate::engine::Engine> {
    state.0.lock().unwrap_or_else(|p| p.into_inner())
}

#[tauri::command]
pub fn snapshot(state: State<AppState>) -> Snapshot {
    engine(&state).snapshot(now_ms())
}

#[tauri::command]
pub fn act(app: AppHandle, action: Action) -> Snapshot {
    let is_preset = matches!(action, Action::Preset(_));
    let snap = shell::dispatch(&app, action);
    if is_preset {
        shell::emit_settings(&app);
    }
    snap
}

#[tauri::command]
pub fn today_stats(state: State<AppState>) -> DayStats {
    engine(&state).today(now_ms())
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Settings {
    engine(&state).settings.clone()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsResult {
    settings: Settings,
    shortcut_error: Option<String>,
}

#[tauri::command]
pub fn set_settings(app: AppHandle, state: State<AppState>, settings: Settings) -> SettingsResult {
    let previous = engine(&state).settings.clone();
    let mut next = settings.sanitized();

    let mut shortcut_error = None;
    if next.hide_shortcut != previous.hide_shortcut {
        if let Err(e) = shell::register_hide_shortcut(&app, &next.hide_shortcut, Some(&previous.hide_shortcut)) {
            next.hide_shortcut = previous.hide_shortcut.clone();
            shortcut_error = Some(e);
        }
    }

    let (snap, saved) = {
        let mut e = engine(&state);
        e.update_settings(next);
        (e.snapshot(now_ms()), e.settings.clone())
    };
    shell::apply_window_settings(&app, &saved);
    shell::apply_app_settings(&app, Some(&previous), &saved);
    shell::broadcast(&app, &snap);
    shell::emit_settings(&app);
    SettingsResult { settings: saved, shortcut_error }
}

#[tauri::command]
pub fn toggle_clock(app: AppHandle) {
    shell::toggle_clock(&app);
}

#[tauri::command]
pub fn open_panel(app: AppHandle, tab: Option<String>) {
    shell::open_panel(&app, tab.as_deref());
}

#[tauri::command]
pub fn show_clock_menu(app: AppHandle) -> Result<(), String> {
    let menu = shell::clock_menu(&app).map_err(|e| e.to_string())?;
    let w = app.get_webview_window(CLOCK).ok_or("no clock window")?;
    w.popup_menu(&menu).map_err(|e| e.to_string())
}

/// The user dragged the resize grip: remember the new scale for this widget.
#[tauri::command]
pub fn set_clock_scale(app: AppHandle, scale: f64) {
    shell::change_settings(&app, |s| s.scale = scale);
}

#[tauri::command]
pub fn test_sound(state: State<AppState>) {
    let s = engine(&state).settings.clone();
    shell::chime(&Settings { sound: true, ..s }, crate::timer::Phase::Focus);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    data_dir: String,
}

#[tauri::command]
pub fn app_info(app: AppHandle) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        data_dir: app.path().app_data_dir().map(|p| p.display().to_string()).unwrap_or_default(),
    }
}

#[tauri::command]
pub fn reveal_data_dir(app: AppHandle) {
    if let Ok(dir) = app.path().app_data_dir() {
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("/usr/bin/open").arg(dir).spawn();
        #[cfg(not(target_os = "macos"))]
        let _ = dir;
    }
}
