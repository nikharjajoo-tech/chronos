//! IPC surface used by the web UI. Thin: each command forwards to the engine
//! or the shell and returns plain data.

use crate::analytics::PeriodStats;
use crate::companion::{self, Quick};
use crate::engine::{now_ms, Action, Snapshot, TodayStats};
use crate::sysmac;
use crate::settings::Settings;
use crate::shell::{self, AppState, CLOCK};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

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
pub fn today_stats(state: State<AppState>) -> TodayStats {
    engine(&state).today_full(now_ms())
}

/// kind: "week" | "month"; offset: 0 = current, -1 = previous, …
#[tauri::command]
pub fn period_stats(state: State<AppState>, kind: String, offset: i32) -> PeriodStats {
    engine(&state).period(&kind, offset, now_ms())
}

#[tauri::command]
pub fn recent_projects(state: State<AppState>) -> Vec<String> {
    engine(&state).db.recent_projects(8).unwrap_or_default()
}

#[tauri::command]
pub fn log_distraction(app: AppHandle) {
    companion::log_distraction(&app);
}

#[tauri::command]
pub fn set_distraction_category(app: AppHandle, state: State<AppState>, id: i64, category: Option<String>) {
    let _ = engine(&state).db.set_distraction_category(id, category.as_deref());
    let _ = app.emit(crate::shell::EV_STATS, ());
}

#[tauri::command]
pub fn rate_session(app: AppHandle, state: State<AppState>, id: i64, rating: Option<u8>, goal: Option<String>) {
    let rating = rating.filter(|r| (1..=4).contains(r));
    let goal = goal.filter(|g| ["yes", "partial", "no"].contains(&g.as_str()));
    let _ = engine(&state).db.rate_session(id, rating, goal.as_deref());
    let _ = app.emit(crate::shell::EV_STATS, ());
}

#[tauri::command]
pub fn quick_state(app: AppHandle) -> Option<Quick> {
    companion::quick_state(&app)
}

#[tauri::command]
pub fn quick_close(app: AppHandle) {
    companion::hide_quick(&app);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperStatus {
    /// Both "Chronos Focus On/Off" shortcuts exist in the Shortcuts app.
    dnd_installed: bool,
    /// Auto-hide apps running right now.
    running: Vec<String>,
}

#[tauri::command]
pub async fn helper_status(state: State<'_, AppState>) -> Result<HelperStatus, ()> {
    let apps = engine(&state).settings.auto_hide_apps.clone();
    let names = sysmac::shortcut_names();
    let has = |n: &str| names.iter().any(|x| x == n);
    Ok(HelperStatus {
        dnd_installed: has(sysmac::FOCUS_ON_SHORTCUT) && has(sysmac::FOCUS_OFF_SHORTCUT),
        running: sysmac::running_matches(&apps, &sysmac::process_names()),
    })
}

#[tauri::command]
pub async fn install_dnd_shortcuts() -> Result<(), String> {
    sysmac::install_focus_shortcuts()
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
    if next.hide_shortcut != previous.hide_shortcut || next.distraction_shortcut != previous.distraction_shortcut {
        if let Err(e) = shell::register_shortcuts(&app, &next, Some(&previous)) {
            next.hide_shortcut = previous.hide_shortcut.clone();
            next.distraction_shortcut = previous.distraction_shortcut.clone();
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
