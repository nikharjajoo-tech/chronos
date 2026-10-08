//! The quiet helpers around the timer: the watchdog (completion, away
//! detection, auto-hide, menu-bar countdown), Do Not Disturb, notifications,
//! and the small popup used for distraction logging and reflections.

use crate::engine::{day_bounds, now_ms, Snapshot};
use crate::settings::Settings;
use crate::shell::{self, CLOCK};
use crate::sysmac;
use crate::timer::{EndStatus, Finished, Phase, RunState, Timer, MINUTE};
use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_notification::NotificationExt;

pub const QUICK: &str = "quick";
pub const EV_QUICK: &str = "chronos://quick";

#[derive(Default)]
pub struct Companion {
    /// The clock is hidden because an auto-hide app is running; bring it back after.
    auto_hidden: bool,
    /// An auto-hide app (screen share, recorder) is running right now.
    sharing: bool,
    /// What we last asked Do Not Disturb to be; None = never touched.
    dnd_on: Option<bool>,
    tray_title: Option<String>,
    quick: Option<Quick>,
}

pub struct CompanionState(pub Mutex<Companion>);

/// What the quick popup is showing.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Quick {
    Distraction { id: i64, count_today: u32, session_number: Option<u32> },
    Reflection { session_id: i64, minutes: i64, task: Option<String>, project: Option<String> },
}

fn comp<R>(app: &AppHandle, f: impl FnOnce(&mut Companion) -> R) -> R {
    let state = app.state::<CompanionState>();
    let mut c = state.0.lock().unwrap_or_else(|p| p.into_inner());
    f(&mut c)
}

pub fn is_sharing(app: &AppHandle) -> bool {
    comp(app, |c| c.sharing)
}

pub fn set_auto_hidden(app: &AppHandle, on: bool) {
    comp(app, |c| c.auto_hidden = on);
}

// ── watchdog ───────────────────────────────────────────────────────────────

/// Runs every 500 ms for the life of the app. Cheap: a clock comparison, an
/// idle-time read during focus, and a process scan once a second.
pub fn spawn_watchdog(app: AppHandle) {
    std::thread::spawn(move || {
        let mut day = day_bounds(now_ms()).0;
        let mut last = now_ms();
        let mut n: u64 = 0;
        loop {
            std::thread::sleep(Duration::from_millis(500));
            n += 1;
            let now = now_ms();
            // A long gap between iterations means the Mac was asleep.
            let gap = now - last;
            last = now;
            let today = day_bounds(now).0;

            let (away, finished, snap, settings, title) = shell::with_engine(&app, |e| {
                let threshold = e.settings.away_min as i64 * MINUTE;
                let mut away = false;
                if threshold > 0 && e.timer.phase == Phase::Focus && e.timer.run == RunState::Running {
                    if gap >= threshold {
                        away = e.away(now - gap, now);
                    } else {
                        let idle = sysmac::idle_ms();
                        if idle >= threshold {
                            away = e.away(now - idle, now);
                        }
                    }
                }
                let f = e.tick(now);
                let changed = away || f.is_some() || today != day;
                let snap = changed.then(|| e.snapshot(now));
                let title = menu_bar_title(&e.timer, now, e.hidden, &e.settings);
                (away, f, snap, e.settings.clone(), title)
            });
            day = today;

            if let Some(snap) = snap {
                if let Some(f) = &finished {
                    on_finished(&app, &settings, f, &snap);
                }
                if away {
                    notify(
                        &app,
                        &settings,
                        "Paused while you were away",
                        &format!(
                            "No activity for {} min, so the focus session stopped counting. Press play when you're back.",
                            snap.away_ms.unwrap_or(0) / MINUTE
                        ),
                    );
                }
                shell::broadcast(&app, &snap);
            }
            set_tray_title(&app, title);
            if n % 2 == 0 {
                auto_hide_check(&app, &settings);
            }
        }
    });
}

/// Sound, then either the reflection popup or a notification.
pub fn on_finished(app: &AppHandle, settings: &Settings, f: &Finished, snap: &Snapshot) {
    if f.status != EndStatus::Completed {
        return;
    }
    shell::chime(settings, f.phase);
    let minutes = (f.actual_ms + MINUTE / 2) / MINUTE;
    if f.phase == Phase::Focus {
        if let (true, false, false, Some(id)) = (settings.reflection, snap.hidden, is_sharing(app), f.session_id) {
            show_quick(
                app,
                Quick::Reflection { session_id: id, minutes, task: f.task.clone(), project: f.project.clone() },
                false,
            );
            return;
        }
        let next = if snap.phase == Phase::LongBreak { "Long break next. You've earned it." } else { "Short break next." };
        notify(app, settings, "Focus session complete", &format!("{minutes} min focused. {next}"));
    } else {
        notify(app, settings, "Break's over", &format!("Ready for session {}?", snap.session_number));
    }
}

pub fn notify(app: &AppHandle, settings: &Settings, title: &str, body: &str) {
    if settings.notifications {
        let _ = app.notification().builder().title(title).body(body).show();
    }
}

// ── menu-bar countdown ─────────────────────────────────────────────────────

pub fn menu_bar_title(t: &Timer, now: i64, hidden: bool, s: &Settings) -> Option<String> {
    let show = match s.menu_bar_timer.as_str() {
        "always" => true,
        "hidden" => hidden,
        _ => false,
    };
    if !show || t.run == RunState::Idle {
        return None;
    }
    let secs = (t.remaining(now).max(0) + 999) / 1000;
    let clock = format!("{}:{:02}", secs / 60, secs % 60);
    let mark = if t.phase == Phase::Focus { "" } else { "☕ " };
    Some(if t.run == RunState::Paused { format!("{mark}⏸ {clock}") } else { format!("{mark}{clock}") })
}

fn set_tray_title(app: &AppHandle, title: Option<String>) {
    let changed = comp(app, |c| {
        if c.tray_title == title {
            false
        } else {
            c.tray_title = title.clone();
            true
        }
    });
    if changed {
        shell::set_tray_title(app, title.as_deref());
    }
}

// ── auto-hide while sharing / recording ────────────────────────────────────

fn auto_hide_check(app: &AppHandle, settings: &Settings) {
    let matched = settings.auto_hide
        && !settings.auto_hide_apps.is_empty()
        && !sysmac::running_matches(&settings.auto_hide_apps, &sysmac::process_names()).is_empty();
    let (hide, show) = comp(app, |c| {
        if matched == c.sharing {
            return (false, false);
        }
        c.sharing = matched;
        (matched, !matched && c.auto_hidden)
    });
    if hide {
        hide_quick(app);
        let visible = app.get_webview_window(CLOCK).and_then(|w| w.is_visible().ok()).unwrap_or(false);
        if visible {
            shell::set_clock_visible(app, false, true);
        }
    } else if show {
        shell::set_clock_visible(app, true, true);
    }
}

// ── Do Not Disturb ─────────────────────────────────────────────────────────

/// On exactly while a focus session is running (and the setting is on).
pub fn sync_dnd(app: &AppHandle, snap: &Snapshot, enabled: bool) {
    let want = enabled && snap.phase == Phase::Focus && snap.run == RunState::Running;
    let changed = comp(app, |c| {
        let differs = match c.dnd_on {
            None => want,
            Some(on) => on != want,
        };
        if differs {
            c.dnd_on = Some(want);
        }
        differs
    });
    if changed {
        sysmac::run_shortcut(if want { sysmac::FOCUS_ON_SHORTCUT } else { sysmac::FOCUS_OFF_SHORTCUT });
    }
}

/// Called on quit: never leave Do Not Disturb stuck on.
pub fn dnd_off_now(app: &AppHandle) {
    if comp(app, |c| c.dnd_on.take()) == Some(true) {
        sysmac::run_shortcut_blocking(sysmac::FOCUS_OFF_SHORTCUT);
    }
}

// ── distractions & the quick popup ─────────────────────────────────────────

pub fn log_distraction(app: &AppHandle) {
    let now = now_ms();
    let (logged, snap) = shell::with_engine(app, |e| (e.log_distraction(now), e.snapshot(now)));
    shell::broadcast(app, &snap);
    // While sharing the screen, log silently: no popup on the recording.
    if let (Some(l), false) = (logged, is_sharing(app)) {
        show_quick(app, Quick::Distraction { id: l.id, count_today: l.count_today, session_number: l.session_number }, true);
    }
}

pub fn quick_state(app: &AppHandle) -> Option<Quick> {
    comp(app, |c| c.quick.clone())
}

pub fn hide_quick(app: &AppHandle) {
    comp(app, |c| c.quick = None);
    if let Some(w) = app.get_webview_window(QUICK) {
        let _ = w.hide();
    }
}

fn show_quick(app: &AppHandle, q: Quick, focus: bool) {
    let (w, h) = match q {
        Quick::Distraction { .. } => (330.0, 146.0),
        Quick::Reflection { .. } => (330.0, 186.0),
    };
    comp(app, |c| c.quick = Some(q.clone()));
    let capture_protected = shell::with_engine(app, |e| e.settings.capture_protected);
    let win = match app.get_webview_window(QUICK) {
        Some(win) => win,
        None => {
            let built = WebviewWindowBuilder::new(app, QUICK, WebviewUrl::App("index.html#quick".into()))
                .title("Chronos")
                .inner_size(w, h)
                .transparent(true)
                .decorations(false)
                .shadow(false)
                .resizable(false)
                .always_on_top(true)
                .visible_on_all_workspaces(true)
                .skip_taskbar(true)
                .accept_first_mouse(true)
                .focused(focus)
                .visible(false)
                .build();
            match built {
                Ok(win) => win,
                Err(_) => return,
            }
        }
    };
    let _ = win.set_size(LogicalSize::new(w, h));
    let _ = win.set_position(quick_position(app, w, h));
    let _ = win.set_content_protected(capture_protected);
    let _ = win.emit(EV_QUICK, &q);
    let _ = win.show();
    if focus {
        let _ = win.set_focus();
    }
}

/// Just below the clock (or above it near the screen bottom); top-right of
/// the screen when the clock is hidden.
fn quick_position(app: &AppHandle, w: f64, h: f64) -> LogicalPosition<f64> {
    let clock = app.get_webview_window(CLOCK);
    if let Some(c) = clock.as_ref().filter(|c| c.is_visible().unwrap_or(false)) {
        if let (Ok(f), Ok(pos), Ok(size), Ok(Some(m))) = (c.scale_factor(), c.outer_position(), c.outer_size(), c.current_monitor()) {
            let (pos, size) = (pos.to_logical::<f64>(f), size.to_logical::<f64>(f));
            let (mpos, msize) = (m.position().to_logical::<f64>(f), m.size().to_logical::<f64>(f));
            let x = (pos.x + size.width - w).clamp(mpos.x + 8.0, mpos.x + msize.width - w - 8.0);
            let below = pos.y + size.height + 4.0;
            let y = if below + h <= mpos.y + msize.height - 8.0 { below } else { (pos.y - h - 4.0).max(mpos.y + 30.0) };
            return LogicalPosition::new(x, y);
        }
    }
    let monitor = clock.as_ref().and_then(|c| c.primary_monitor().ok().flatten());
    match monitor {
        Some(m) => {
            let f = m.scale_factor();
            let (mpos, msize) = (m.position().to_logical::<f64>(f), m.size().to_logical::<f64>(f));
            LogicalPosition::new(mpos.x + msize.width - w - 16.0, mpos.y + 34.0)
        }
        None => LogicalPosition::new(40.0, 40.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(mode: &str) -> Settings {
        Settings { menu_bar_timer: mode.into(), ..Default::default() }
    }

    #[test]
    fn menu_bar_title_modes() {
        let mut t = Timer::new(25 * MINUTE);
        assert_eq!(menu_bar_title(&t, 0, true, &settings("always")), None, "idle shows nothing");
        t.start(0);
        let now = 6 * MINUTE + 18_000;
        assert_eq!(menu_bar_title(&t, now, false, &settings("always")).as_deref(), Some("18:42"));
        assert_eq!(menu_bar_title(&t, now, false, &settings("hidden")), None);
        assert_eq!(menu_bar_title(&t, now, true, &settings("hidden")).as_deref(), Some("18:42"));
        assert_eq!(menu_bar_title(&t, now, true, &settings("off")), None);
        t.pause(now);
        assert_eq!(menu_bar_title(&t, now, true, &settings("always")).as_deref(), Some("⏸ 18:42"));
    }

    #[test]
    fn quick_payload_shape() {
        let q = Quick::Distraction { id: 3, count_today: 2, session_number: Some(4) };
        assert_eq!(
            serde_json::to_value(&q).unwrap(),
            serde_json::json!({ "kind": "distraction", "id": 3, "countToday": 2, "sessionNumber": 4 })
        );
    }
}
