//! IPC surface used by the web UI. Thin: each command forwards to the engine
//! or the shell and returns plain data.

use crate::analytics::PeriodStats;
use crate::companion::{self, Quick};
use crate::engine::{now_ms, Action, InsightsView, Snapshot, TaskPatch, TodayStats};
use crate::tasks::{self, Bucket, ParsedTask, TaskRow};
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

/// Open tasks, plus tasks done in the last `done_days` local days.
#[tauri::command]
pub fn tasks_list(state: State<AppState>, done_days: u32) -> Vec<TaskRow> {
    engine(&state).tasks_list(done_days, now_ms())
}

/// For the preview chips while typing; writes nothing.
#[tauri::command]
pub fn task_parse(text: String) -> Option<ParsedTask> {
    tasks::parse(&text)
}

#[tauri::command]
pub fn task_add(app: AppHandle, state: State<AppState>, text: String, bucket: Bucket, top: bool) -> Result<TaskRow, String> {
    let row = engine(&state).task_add(&text, bucket, top, now_ms())?;
    shell::tasks_changed(&app);
    Ok(row)
}

#[tauri::command]
pub fn task_update(app: AppHandle, state: State<AppState>, id: i64, patch: TaskPatch) -> Result<TaskRow, String> {
    let row = engine(&state).task_update(id, patch, now_ms())?;
    shell::tasks_changed(&app);
    Ok(row)
}

#[tauri::command]
pub fn task_move(app: AppHandle, state: State<AppState>, id: i64, bucket: Bucket, before_id: Option<i64>) -> Result<(), String> {
    engine(&state).task_move(id, bucket, before_id, now_ms())?;
    shell::tasks_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn task_set_done(app: AppHandle, state: State<AppState>, id: i64, done: bool) -> Result<(), String> {
    engine(&state).task_set_done(id, done, now_ms())?;
    shell::tasks_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn task_delete(app: AppHandle, state: State<AppState>, id: i64) -> Result<(), String> {
    engine(&state).task_delete(id, now_ms())?;
    shell::tasks_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn task_undo_delete(app: AppHandle, state: State<AppState>) -> Option<TaskRow> {
    let row = engine(&state).task_undo_delete(now_ms());
    if row.is_some() {
        shell::tasks_changed(&app);
    }
    row
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
pub fn insights(state: State<AppState>) -> InsightsView {
    engine(&state).insights_view(now_ms())
}

/// Accept or decline the current suggestion; returns the refreshed view.
#[tauri::command]
pub fn respond_suggestion(app: AppHandle, state: State<AppState>, id: String, accept: bool) -> InsightsView {
    let (changed, view, snap, settings) = {
        let mut e = engine(&state);
        let now = now_ms();
        let changed = e.respond_suggestion(&id, accept, now);
        (changed, e.insights_view(now), e.snapshot(now), e.settings.clone())
    };
    if changed {
        shell::apply_window_settings(&app, &settings);
        shell::broadcast(&app, &snap);
        shell::emit_settings(&app);
    }
    view
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailStatus {
    has_password: bool,
    status: crate::report::ReportStatus,
    /// Next scheduled send, epoch ms.
    next_send: i64,
}

#[tauri::command]
pub async fn email_status(app: AppHandle) -> Result<EmailStatus, ()> {
    let settings = app.state::<AppState>().0.lock().map(|e| e.settings.clone()).unwrap_or_default();
    let has_password = !settings.email_from.is_empty() && crate::report::keychain_get(&settings.email_from).is_some();
    use chrono::{Datelike, Duration, Local, TimeZone};
    let now = Local::now();
    let monday = now.date_naive() - Duration::days(now.weekday().num_days_from_monday() as i64);
    let at = |d: chrono::NaiveDate| {
        Local
            .from_local_datetime(&d.and_hms_opt(settings.email_hour, 0, 0).unwrap_or_default())
            .earliest()
            .map(|t| t.timestamp_millis())
            .unwrap_or(0)
    };
    let this_week = at(monday + Duration::days(settings.email_day as i64));
    let next_send = if this_week > now.timestamp_millis() { this_week } else { at(monday + Duration::days(7 + settings.email_day as i64)) };
    Ok(EmailStatus { has_password, status: companion::report_status(&app), next_send })
}

#[tauri::command]
pub fn set_email_password(state: State<AppState>, password: String) -> Result<(), String> {
    let from = engine(&state).settings.email_from.clone();
    if from.is_empty() {
        return Err("Enter your Gmail address first.".into());
    }
    crate::report::keychain_set(&from, password.trim())
}

#[tauri::command]
pub async fn send_test_email(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || companion::send_test_report(&app))
        .await
        .map_err(|e| e.to_string())?
}

/// Write the email to a file and open it in the default browser.
#[tauri::command]
pub fn preview_email(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    let now = now_ms();
    let (mut email, empty) = engine(&state).render_report(-1, now);
    if empty {
        email = engine(&state).render_report(0, now).0;
    }
    let dir = app.path().app_cache_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("weekly-email-preview.html");
    let html = email.html.replacen("<body", &format!("<title>{}</title><body", email.subject.replace('<', "&lt;")), 1);
    std::fs::write(&path, html).map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("/usr/bin/open").arg(&path).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn open_app_passwords() {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("/usr/bin/open").arg("https://myaccount.google.com/apppasswords").spawn();
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
    if next.hide_shortcut != previous.hide_shortcut
        || next.distraction_shortcut != previous.distraction_shortcut
        || next.quick_add_shortcut != previous.quick_add_shortcut
    {
        if let Err(e) = shell::register_shortcuts(&app, &next, Some(&previous)) {
            next.hide_shortcut = previous.hide_shortcut.clone();
            next.distraction_shortcut = previous.distraction_shortcut.clone();
            next.quick_add_shortcut = previous.quick_add_shortcut.clone();
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

/// The layout the Plan tab opens in: "panel" or "board".
#[tauri::command]
pub fn plan_mode(app: AppHandle) -> String {
    shell::plan_mode(&app)
}

#[tauri::command]
pub fn set_plan_mode(app: AppHandle, mode: String) {
    shell::set_plan_mode(&app, &mode);
}

/// Widen the panel window into the board (true) or shrink it back.
#[tauri::command]
pub fn set_panel_layout(app: AppHandle, board: bool) {
    shell::set_panel_layout(&app, board);
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
