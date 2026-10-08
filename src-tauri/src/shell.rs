//! Everything that touches the desktop: windows, tray, native menus, global
//! shortcuts and sound. Background behaviour lives in `companion`.

use crate::companion;
use crate::engine::{now_ms, Action, Engine, Snapshot};
use crate::settings::{widget_base_size, Settings, PRESETS, SCALE_RANGE, STYLES, WIDGETS};
use crate::timer::{Phase, RunState, MINUTE};
use chrono::{Local, TimeZone};
use std::sync::Mutex;
use tauri::menu::{CheckMenuItem, Menu, MenuBuilder, MenuEvent, MenuItem, PredefinedMenuItem, SubmenuBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder, Wry};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

pub const CLOCK: &str = "clock";
pub const PANEL: &str = "panel";
pub const EV_SNAPSHOT: &str = "chronos://snapshot";
pub const EV_SETTINGS: &str = "chronos://settings";
/// Stored stats changed outside a snapshot (rating, distraction category).
pub const EV_STATS: &str = "chronos://stats";
const TRAY: &str = "tray";

pub struct AppState(pub Mutex<Engine>);

/// Tray items whose labels follow the timer state.
pub struct TrayItems {
    toggle_clock: MenuItem<Wry>,
    toggle_run: MenuItem<Wry>,
}

/// Only position is remembered by the plugin; size follows the widget
/// layout and scale stored in settings.
pub fn window_state_flags() -> StateFlags {
    StateFlags::POSITION
}

pub fn with_engine<R>(app: &AppHandle, f: impl FnOnce(&mut Engine) -> R) -> R {
    let state = app.state::<AppState>();
    let mut engine = state.0.lock().unwrap_or_else(|p| p.into_inner());
    f(&mut engine)
}

/// Apply an action from any source (UI, tray, menu) and tell every window.
pub fn dispatch(app: &AppHandle, action: Action) -> Snapshot {
    let (finished, snap, settings) = with_engine(app, |e| {
        let now = now_ms();
        let f = e.apply(action, now);
        (f, e.snapshot(now), e.settings.clone())
    });
    if let Some(f) = &finished {
        companion::on_finished(app, &settings, f, &snap);
    }
    broadcast(app, &snap);
    snap
}

/// Tell every window about a new state, and keep the tray and Do Not
/// Disturb in step with it.
pub fn broadcast(app: &AppHandle, snap: &Snapshot) {
    let _ = app.emit(EV_SNAPSHOT, snap);
    refresh_tray(app, snap);
    let dnd = with_engine(app, |e| e.settings.dnd);
    companion::sync_dnd(app, snap, dnd);
}

/// Completion sound. Played from Rust so it works while the clock is hidden.
pub fn chime(settings: &Settings, finished_phase: Phase) {
    if !settings.sound || settings.volume <= 0.0 {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let name = if finished_phase == Phase::Focus { "Glass" } else { "Hero" };
        let child = std::process::Command::new("/usr/bin/afplay")
            .args(["-v", &format!("{:.2}", settings.volume), &format!("/System/Library/Sounds/{name}.aiff")])
            .spawn();
        if let Ok(mut child) = child {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = finished_phase;
}

// ── windows ────────────────────────────────────────────────────────────────

/// Bring the clock back if hidden (used when the app icon is clicked again).
pub fn show_clock(app: &AppHandle) {
    let Some(w) = app.get_webview_window(CLOCK) else { return };
    if !w.is_visible().unwrap_or(true) {
        toggle_clock(app);
    } else {
        open_panel(app, Some("today"));
    }
}

pub fn toggle_clock(app: &AppHandle) {
    let Some(w) = app.get_webview_window(CLOCK) else { return };
    let visible = w.is_visible().unwrap_or(true);
    set_clock_visible(app, !visible, false);
}

/// Show or hide the clock. `auto` marks a hide done by auto-hide (so it can
/// undo it later); any manual change takes over from auto-hide.
pub fn set_clock_visible(app: &AppHandle, visible: bool, auto: bool) {
    let Some(w) = app.get_webview_window(CLOCK) else { return };
    if visible {
        let _ = w.show();
    } else {
        let _ = app.save_window_state(window_state_flags());
        let _ = w.hide();
        companion::hide_quick(app);
    }
    companion::set_auto_hidden(app, auto && !visible);
    let snap = with_engine(app, |e| {
        e.hidden = !visible;
        e.snapshot(now_ms())
    });
    broadcast(app, &snap);
}

pub fn open_panel(app: &AppHandle, tab: Option<&str>) {
    if let Some(w) = app.get_webview_window(PANEL) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        if let Some(tab) = tab {
            let _ = w.emit("chronos://tab", tab);
        }
        return;
    }
    let url = format!("index.html#{}", tab.unwrap_or("today"));
    let built = WebviewWindowBuilder::new(app, PANEL, WebviewUrl::App(url.into()))
        .title("Chronos")
        .inner_size(480.0, 680.0)
        .min_inner_size(420.0, 520.0)
        .resizable(true)
        .build();
    if let Ok(w) = built {
        let _ = w.set_focus();
    }
}

/// Window-level settings: stacking, spaces, capture protection, size.
pub fn apply_window_settings(app: &AppHandle, s: &Settings) {
    if let Some(w) = app.get_webview_window(CLOCK) {
        let _ = w.set_always_on_top(s.always_on_top);
        let _ = w.set_visible_on_all_workspaces(s.all_workspaces);
        let _ = w.set_content_protected(s.capture_protected);
        size_clock(&w, s);
    }
}

/// Size the clock window for its widget layout and scale. Resizing keeps the
/// edge nearest the screen side fixed, so a clock parked in the top-right
/// corner stays there when it switches from Large to Pill.
fn size_clock(w: &tauri::WebviewWindow, s: &Settings) {
    let (bw, bh) = widget_base_size(&s.widget);
    let target = LogicalSize::new((bw * s.scale).round(), (bh * s.scale).round());
    let _ = w.set_min_size(Some(LogicalSize::new(bw * SCALE_RANGE.0, bh * SCALE_RANGE.0)));
    let _ = w.set_max_size(Some(LogicalSize::new(bw * SCALE_RANGE.1, bh * SCALE_RANGE.1)));

    let Ok(factor) = w.scale_factor() else { return };
    let (Ok(pos), Ok(size)) = (w.outer_position(), w.outer_size()) else { return };
    let pos = pos.to_logical::<f64>(factor);
    let size = size.to_logical::<f64>(factor);
    if (size.width - target.width).abs() < 1.0 && (size.height - target.height).abs() < 1.0 {
        return;
    }
    let (mut x, mut y) = (pos.x, pos.y);
    if let Ok(Some(m)) = w.current_monitor() {
        let mpos = m.position().to_logical::<f64>(factor);
        let msize = m.size().to_logical::<f64>(factor);
        if pos.x + size.width / 2.0 > mpos.x + msize.width / 2.0 {
            x = pos.x + size.width - target.width;
        }
        if pos.y + size.height / 2.0 > mpos.y + msize.height / 2.0 {
            y = pos.y + size.height - target.height;
        }
    }
    let _ = w.set_size(target);
    let _ = w.set_position(LogicalPosition::new(x, y));
}

/// App-level settings: open at login, Dock icon. Only touches what changed.
pub fn apply_app_settings(app: &AppHandle, prev: Option<&Settings>, s: &Settings) {
    if prev.map(|p| p.launch_at_login) != Some(s.launch_at_login) || prev.is_none() {
        let auto = app.autolaunch();
        // Re-enabling on every launch also refreshes the path if the app moved.
        let _ = if s.launch_at_login { auto.enable() } else { auto.disable() };
    }
    #[cfg(target_os = "macos")]
    if prev.map(|p| p.show_in_dock) != Some(s.show_in_dock) {
        let _ = app.set_activation_policy(if s.show_in_dock {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        });
    }
}

/// Apply a settings change from a menu and tell every window.
pub fn change_settings(app: &AppHandle, f: impl FnOnce(&mut Settings)) {
    let (prev, next) = with_engine(app, |e| {
        let prev = e.settings.clone();
        let mut next = prev.clone();
        f(&mut next);
        e.update_settings(next);
        (prev, e.settings.clone())
    });
    apply_window_settings(app, &next);
    apply_app_settings(app, Some(&prev), &next);
    emit_settings(app);
}

/// First launch: park the clock in the top-right corner of the main screen.
pub fn place_clock_first_run(app: &AppHandle) {
    let Some(w) = app.get_webview_window(CLOCK) else { return };
    let Ok(Some(monitor)) = w.primary_monitor() else { return };
    let scale = monitor.scale_factor();
    let size = monitor.size().to_logical::<f64>(scale);
    let origin = monitor.position().to_logical::<f64>(scale);
    let win = w.outer_size().map(|s| s.to_logical::<f64>(scale)).unwrap_or(LogicalSize::new(240.0, 300.0));
    let _ = w.set_position(LogicalPosition::new(
        origin.x + size.width - win.width - 24.0,
        origin.y + 56.0,
    ));
}

// ── global shortcuts ───────────────────────────────────────────────────────

/// (Re)register the hide/show and log-distraction shortcuts. On failure the
/// previous pair is restored so the user is never left without them.
pub fn register_shortcuts(app: &AppHandle, s: &Settings, previous: Option<&Settings>) -> Result<(), String> {
    let gs = app.global_shortcut();
    let bind = |hide: &str, distraction: &str| -> Result<(), String> {
        let _ = gs.unregister_all();
        gs.on_shortcut(hide, |app, _, event| {
            if event.state == ShortcutState::Pressed {
                toggle_clock(app);
            }
        })
        .map_err(|e| format!("Couldn't use “{}” to hide the clock: {e}", pretty_shortcut(hide)))?;
        gs.on_shortcut(distraction, |app, _, event| {
            if event.state == ShortcutState::Pressed {
                companion::log_distraction(app);
            }
        })
        .map_err(|e| format!("Couldn't use “{}” to log distractions: {e}", pretty_shortcut(distraction)))
    };
    match bind(&s.hide_shortcut, &s.distraction_shortcut) {
        Ok(()) => Ok(()),
        Err(err) => {
            if let Some(p) = previous {
                let _ = bind(&p.hide_shortcut, &p.distraction_shortcut);
            }
            Err(err)
        }
    }
}

/// "CommandOrControl+Shift+H" → "⌘⇧H"
pub fn pretty_shortcut(s: &str) -> String {
    s.split('+')
        .map(|part| match part.trim().to_ascii_lowercase().as_str() {
            "commandorcontrol" | "cmdorctrl" | "command" | "cmd" | "super" | "meta" => "⌘".to_string(),
            "shift" => "⇧".into(),
            "alt" | "option" => "⌥".into(),
            "control" | "ctrl" => "⌃".into(),
            _ => part.trim().to_uppercase(),
        })
        .collect()
}

// ── menus ──────────────────────────────────────────────────────────────────

fn run_label(snap: &Snapshot) -> &'static str {
    match (snap.run, snap.phase) {
        (RunState::Running, _) => "Pause",
        (RunState::Paused, _) => "Resume",
        (RunState::Idle, Phase::Focus) => "Start Focus",
        (RunState::Idle, _) => "Start Break",
    }
}

fn phase_label(p: Phase) -> &'static str {
    match p {
        Phase::Focus => "Focus",
        Phase::ShortBreak => "Short Break",
        Phase::LongBreak => "Long Break",
    }
}

fn fmt_clock(ms: i64) -> String {
    let s = (ms.max(0) + 999) / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

fn tray_tooltip(snap: &Snapshot) -> String {
    let phase = phase_label(snap.phase);
    match (snap.run, snap.end_at) {
        (RunState::Running, Some(end)) => {
            let at = Local.timestamp_millis_opt(end).single().map(|t| t.format("%-I:%M %p").to_string());
            format!("Chronos · {phase} until {}", at.unwrap_or_default())
        }
        (RunState::Paused, _) => format!("Chronos · {phase} paused ({} left)", fmt_clock(snap.remaining_ms)),
        _ => format!("Chronos · {phase} ready ({})", fmt_clock(snap.remaining_ms)),
    }
}

pub fn build_tray(app: &AppHandle, shortcut: &str) -> tauri::Result<()> {
    let toggle_clock = MenuItem::with_id(app, "clock:toggle", format!("Hide Clock    {}", pretty_shortcut(shortcut)), true, None::<&str>)?;
    let toggle_run = MenuItem::with_id(app, "act:toggle", "Start Focus", true, None::<&str>)?;
    let menu = MenuBuilder::new(app)
        .item(&toggle_clock)
        .separator()
        .item(&toggle_run)
        .item(&MenuItem::with_id(app, "act:skip", "Skip", true, None::<&str>)?)
        .item(&MenuItem::with_id(app, "act:add5", "Add 5 Minutes", true, None::<&str>)?)
        .item(&MenuItem::with_id(app, "distraction", "Log a Distraction", true, None::<&str>)?)
        .separator()
        .item(&MenuItem::with_id(app, "panel:today", "Today’s Stats…", true, None::<&str>)?)
        .item(&MenuItem::with_id(app, "panel:history", "History…", true, None::<&str>)?)
        .item(&MenuItem::with_id(app, "panel:insights", "Insights…", true, None::<&str>)?)
        .item(&MenuItem::with_id(app, "panel:settings", "Settings…", true, None::<&str>)?)
        .separator()
        .item(&MenuItem::with_id(app, "quit", "Quit Chronos", true, None::<&str>)?)
        .build()?;

    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    TrayIconBuilder::with_id(TRAY)
        .icon(icon)
        .icon_as_template(true)
        .tooltip("Chronos")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .build(app)?;
    app.manage(TrayItems { toggle_clock, toggle_run });
    Ok(())
}

pub fn set_tray_title(app: &AppHandle, title: Option<&str>) {
    if let Some(tray) = app.tray_by_id(TRAY) {
        let _ = tray.set_title(title);
    }
}

fn refresh_tray(app: &AppHandle, snap: &Snapshot) {
    if let Some(items) = app.try_state::<TrayItems>() {
        let shortcut = with_engine(app, |e| e.settings.hide_shortcut.clone());
        let verb = if snap.hidden { "Show Clock" } else { "Hide Clock" };
        let _ = items.toggle_clock.set_text(format!("{verb}    {}", pretty_shortcut(&shortcut)));
        let _ = items.toggle_run.set_text(run_label(snap));
    }
    if let Some(tray) = app.tray_by_id(TRAY) {
        let _ = tray.set_tooltip(Some(tray_tooltip(snap)));
    }
}

/// The ⋯ / right-click menu on the clock. Built on demand so it always
/// reflects the current state; native, so it can extend past the tiny window.
pub fn clock_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let (snap, settings) = with_engine(app, |e| (e.snapshot(now_ms()), e.settings.clone()));
    let item = |id: &str, text: &str, enabled: bool| MenuItem::with_id(app, id, text, enabled, None::<&str>);
    let started = snap.run != RunState::Idle;
    let skip_label = if snap.phase == Phase::Focus { "Skip to Break" } else { "Skip to Focus" };

    let mut switch = SubmenuBuilder::new(app, "Switch To");
    for p in [Phase::Focus, Phase::ShortBreak, Phase::LongBreak] {
        switch = switch.item(&CheckMenuItem::with_id(
            app,
            format!("phase:{}", p.as_str()),
            phase_label(p),
            true,
            snap.phase == p,
            None::<&str>,
        )?);
    }
    let mut presets = SubmenuBuilder::new(app, "Focus Mode");
    for p in PRESETS {
        presets = presets.item(&CheckMenuItem::with_id(
            app,
            format!("preset:{}", p.id),
            p.label,
            true,
            settings.preset == p.id,
            None::<&str>,
        )?);
    }
    presets = presets.separator().item(&item("panel:settings", "Custom…", true)?);

    let mut widgets = SubmenuBuilder::new(app, "Widget Size");
    for (id, label, ..) in WIDGETS {
        widgets = widgets.item(&CheckMenuItem::with_id(app, format!("widget:{id}"), *label, true, settings.widget == *id, None::<&str>)?);
    }
    widgets = widgets.separator();
    for (id, label) in STYLES {
        widgets = widgets.item(&CheckMenuItem::with_id(app, format!("style:{id}"), format!("{label} Style"), true, settings.style == *id, None::<&str>)?);
    }

    MenuBuilder::new(app)
        .item(&item("act:toggle", run_label(&snap), true)?)
        .item(&item("act:restart", "Restart Session", started)?)
        .item(&item("act:skip", skip_label, true)?)
        .item(&item("act:reset", "Reset", started || snap.remaining_ms != snap.duration_ms)?)
        .separator()
        .item(&item("act:add5", "Add 5 Minutes", true)?)
        .item(&item("act:add10", "Add 10 Minutes", true)?)
        .item(&item("act:sub5", "Remove 5 Minutes", true)?)
        .item(&item("distraction", &format!("Log a Distraction    {}", pretty_shortcut(&settings.distraction_shortcut)), true)?)
        .item(&switch.build()?)
        .item(&presets.build()?)
        .separator()
        .item(&widgets.build()?)
        .item(&CheckMenuItem::with_id(app, "aot", "Always on Top", true, settings.always_on_top, None::<&str>)?)
        .item(&item("clock:toggle", &format!("Hide Clock    {}", pretty_shortcut(&settings.hide_shortcut)), true)?)
        .separator()
        .item(&item("panel:today", "Today’s Stats…", true)?)
        .item(&item("panel:history", "History…", true)?)
        .item(&item("panel:insights", "Insights…", true)?)
        .item(&item("panel:settings", "Settings…", true)?)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&item("quit", "Quit Chronos", true)?)
        .build()
}

pub fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref();
    let action = match id {
        "act:toggle" => Some(Action::Toggle),
        "act:restart" => Some(Action::Restart),
        "act:skip" => Some(Action::Skip),
        "act:reset" => Some(Action::Reset),
        "act:add5" => Some(Action::Adjust(5 * MINUTE)),
        "act:add10" => Some(Action::Adjust(10 * MINUTE)),
        "act:sub5" => Some(Action::Adjust(-5 * MINUTE)),
        _ => None,
    };
    if let Some(a) = action {
        dispatch(app, a);
        return;
    }
    if let Some(p) = id.strip_prefix("phase:").and_then(Phase::parse) {
        dispatch(app, Action::SwitchPhase(p));
    } else if let Some(p) = id.strip_prefix("preset:") {
        dispatch(app, Action::Preset(p.to_string()));
        emit_settings(app);
    } else if let Some(w) = id.strip_prefix("widget:") {
        let w = w.to_string();
        change_settings(app, |s| s.widget = w);
    } else if let Some(st) = id.strip_prefix("style:") {
        let st = st.to_string();
        change_settings(app, |s| s.style = st);
    } else if let Some(tab) = id.strip_prefix("panel:") {
        open_panel(app, Some(tab));
    } else if id == "clock:toggle" {
        toggle_clock(app);
    } else if id == "distraction" {
        companion::log_distraction(app);
    } else if id == "aot" {
        change_settings(app, |s| s.always_on_top = !s.always_on_top);
    } else if id == "quit" {
        companion::dnd_off_now(app);
        let _ = app.save_window_state(window_state_flags());
        app.exit(0);
    }
}

pub fn emit_settings(app: &AppHandle) {
    let s = with_engine(app, |e| e.settings.clone());
    let _ = app.emit(EV_SETTINGS, s);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_shortcuts() {
        assert_eq!(pretty_shortcut("CommandOrControl+Shift+H"), "⌘⇧H");
        assert_eq!(pretty_shortcut("Alt+Control+t"), "⌥⌃T");
    }

    #[test]
    fn clock_format_rounds_up() {
        assert_eq!(fmt_clock(18 * MINUTE + 41_001), "18:42");
        assert_eq!(fmt_clock(0), "0:00");
    }
}
