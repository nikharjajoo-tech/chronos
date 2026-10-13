//! User settings. Stored as one JSON document; `#[serde(default)]` means a
//! settings file written by an older version still loads after new fields
//! are added.

use crate::timer::{Durations, MINUTE};
use serde::{Deserialize, Serialize};

pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub focus_min: u32,
    pub short_min: u32,
    pub long_min: u32,
}

pub const PRESETS: &[Preset] = &[
    Preset { id: "standard", label: "Standard Pomodoro (25 / 5)", focus_min: 25, short_min: 5, long_min: 15 },
    Preset { id: "deep", label: "Deep Work (50 / 10)", focus_min: 50, short_min: 10, long_min: 20 },
    Preset { id: "quick", label: "Quick Focus (15 / 5)", focus_min: 15, short_min: 5, long_min: 15 },
];

pub const MAX_MINUTES: u32 = 240;

/// Widget layouts and their base window size (logical px) at scale 1.0.
/// Must match `LAYOUTS` in src/clock/Clock.tsx.
pub const WIDGETS: &[(&str, &str, f64, f64)] = &[
    ("pill", "Pill", 236.0, 68.0),
    ("small", "Small", 172.0, 172.0),
    ("medium", "Medium", 340.0, 164.0),
    ("large", "Large", 240.0, 300.0),
];
/// Zoom's screen-share helper, the macOS screenshot/recording UI (⌘⇧5) and OBS.
pub const DEFAULT_AUTO_HIDE_APPS: &[&str] = &["CptHost", "screencaptureui", "obs"];
pub const MENU_BAR_TIMER: &[&str] = &["off", "hidden", "always"];

pub const STYLES: &[(&str, &str)] = &[("retro", "Retro"), ("minimal", "Minimal")];
/// Look of the panel, board and their pop-ups. Light or dark follows macOS.
pub const THEMES: &[(&str, &str)] = &[("glass", "Glass"), ("retro", "Retro")];
pub const SCALE_RANGE: (f64, f64) = (0.6, 2.0);

pub fn widget_base_size(widget: &str) -> (f64, f64) {
    WIDGETS
        .iter()
        .find(|w| w.0 == widget)
        .map(|w| (w.2, w.3))
        .unwrap_or((240.0, 300.0))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// "standard" | "deep" | "quick" | "custom"
    pub preset: String,
    pub focus_min: u32,
    pub short_min: u32,
    pub long_min: u32,
    pub long_every: u32,
    pub auto_start_breaks: bool,
    pub auto_start_focus: bool,

    pub always_on_top: bool,
    pub all_workspaces: bool,
    pub opacity: f64,
    /// Hide the clock from screenshots / screen recordings while still visible to you.
    pub capture_protected: bool,
    pub hide_shortcut: String,
    /// "pill" | "small" | "medium" | "large"
    pub widget: String,
    /// Clock style: "retro" | "minimal"
    pub style: String,
    /// Panel and board theme: "glass" | "retro"
    pub theme: String,
    /// Multiplier on the widget's base size.
    pub scale: f64,

    pub launch_at_login: bool,
    pub show_in_dock: bool,

    /// Auto-pause focus after this many idle minutes (0 = off).
    pub away_min: u32,
    /// Hide the clock while any of `auto_hide_apps` is running.
    pub auto_hide: bool,
    /// Process names, matched case-insensitively.
    pub auto_hide_apps: Vec<String>,
    /// Run the "Chronos Focus On/Off" shortcuts around focus sessions.
    pub dnd: bool,
    /// "off" | "hidden" | "always"
    pub menu_bar_timer: String,
    pub distraction_shortcut: String,
    /// Ask "how did it go?" after a focus session.
    pub reflection: bool,
    pub notifications: bool,
    /// Occasional one-line nudges on the clock ("take a proper break").
    pub companion: bool,

    /// Weekly summary by email (opt-in). The Gmail app password lives in
    /// the macOS Keychain, never here.
    pub email_report: bool,
    pub email_to: String,
    /// The Gmail account that sends it.
    pub email_from: String,
    /// 0 = Monday … 6 = Sunday
    pub email_day: u32,
    /// Local hour, 0–23.
    pub email_hour: u32,

    pub sound: bool,
    pub volume: f64,

    pub daily_goal_min: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            preset: "standard".into(),
            focus_min: 25,
            short_min: 5,
            long_min: 15,
            long_every: 4,
            auto_start_breaks: false,
            auto_start_focus: false,
            always_on_top: true,
            all_workspaces: true,
            opacity: 1.0,
            capture_protected: false,
            hide_shortcut: "CommandOrControl+Shift+H".into(),
            widget: "large".into(),
            style: "retro".into(),
            theme: "glass".into(),
            scale: 1.0,
            launch_at_login: true,
            show_in_dock: false,
            away_min: 5,
            auto_hide: true,
            auto_hide_apps: DEFAULT_AUTO_HIDE_APPS.iter().map(|s| s.to_string()).collect(),
            dnd: false,
            menu_bar_timer: "off".into(),
            distraction_shortcut: "Control+Alt+D".into(),
            reflection: true,
            notifications: true,
            companion: true,
            email_report: false,
            email_to: String::new(),
            email_from: String::new(),
            email_day: 0,
            email_hour: 9,
            sound: true,
            volume: 0.6,
            daily_goal_min: 180,
        }
    }
}

impl Settings {
    pub fn durations(&self) -> Durations {
        Durations {
            focus_ms: self.focus_min as i64 * MINUTE,
            short_ms: self.short_min as i64 * MINUTE,
            long_ms: self.long_min as i64 * MINUTE,
            long_every: self.long_every,
        }
    }

    /// Clamp everything into a sane range so a bad value from the UI (or a
    /// hand-edited database) can never wedge the timer.
    pub fn sanitized(mut self) -> Self {
        let clamp_min = |v: u32| v.clamp(1, MAX_MINUTES);
        self.focus_min = clamp_min(self.focus_min);
        self.short_min = clamp_min(self.short_min);
        self.long_min = clamp_min(self.long_min);
        self.long_every = self.long_every.clamp(1, 12);
        self.opacity = if self.opacity.is_finite() { self.opacity.clamp(0.25, 1.0) } else { 1.0 };
        self.volume = if self.volume.is_finite() { self.volume.clamp(0.0, 1.0) } else { 0.6 };
        self.daily_goal_min = self.daily_goal_min.clamp(0, 24 * 60);
        if !WIDGETS.iter().any(|w| w.0 == self.widget) {
            self.widget = "large".into();
        }
        if !STYLES.iter().any(|s| s.0 == self.style) {
            self.style = "retro".into();
        }
        if !THEMES.iter().any(|t| t.0 == self.theme) {
            self.theme = "glass".into();
        }
        self.scale = if self.scale.is_finite() { self.scale.clamp(SCALE_RANGE.0, SCALE_RANGE.1) } else { 1.0 };
        if self.hide_shortcut.trim().is_empty() {
            self.hide_shortcut = Settings::default().hide_shortcut;
        }
        if self.distraction_shortcut.trim().is_empty() {
            self.distraction_shortcut = Settings::default().distraction_shortcut;
        }
        self.away_min = self.away_min.min(60);
        self.email_day = self.email_day.min(6);
        self.email_hour = self.email_hour.min(23);
        self.email_to = self.email_to.trim().chars().take(254).collect();
        self.email_from = self.email_from.trim().chars().take(254).collect();
        if !MENU_BAR_TIMER.contains(&self.menu_bar_timer.as_str()) {
            self.menu_bar_timer = "off".into();
        }
        let mut apps: Vec<String> = Vec::new();
        for a in self.auto_hide_apps.iter().map(|a| a.trim()).filter(|a| !a.is_empty()) {
            if !apps.iter().any(|x| x.eq_ignore_ascii_case(a)) && apps.len() < 24 {
                apps.push(a.chars().take(64).collect());
            }
        }
        self.auto_hide_apps = apps;
        self.preset = match PRESETS.iter().find(|p| {
            p.focus_min == self.focus_min && p.short_min == self.short_min && p.long_min == self.long_min
        }) {
            Some(p) => p.id.into(),
            None => "custom".into(),
        };
        self
    }

    pub fn apply_preset(&mut self, id: &str) -> bool {
        match PRESETS.iter().find(|p| p.id == id) {
            Some(p) => {
                self.focus_min = p.focus_min;
                self.short_min = p.short_min;
                self.long_min = p.long_min;
                self.preset = p.id.into();
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_documents_still_load() {
        let s: Settings = serde_json::from_str(r#"{"focusMin": 40}"#).unwrap();
        assert_eq!(s.focus_min, 40);
        assert_eq!(s.short_min, 5);
        assert!(s.always_on_top);
    }

    #[test]
    fn sanitize_clamps_and_detects_preset() {
        let s = Settings {
            focus_min: 0,
            opacity: 7.0,
            volume: f64::NAN,
            widget: "huge".into(),
            style: "neon".into(),
            theme: "paper".into(),
            scale: 9.0,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(s.widget, "large");
        assert_eq!(s.style, "retro");
        assert_eq!(s.theme, "glass");
        assert_eq!(s.scale, SCALE_RANGE.1);
        let s2 = Settings {
            auto_hide_apps: vec![" Keynote ".into(), "keynote".into(), "".into()],
            menu_bar_timer: "sometimes".into(),
            away_min: 500,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(s2.auto_hide_apps, vec!["Keynote".to_string()]);
        assert_eq!(s2.menu_bar_timer, "off");
        assert_eq!(s2.away_min, 60);
        assert_eq!(s.focus_min, 1);
        assert_eq!(s.opacity, 1.0);
        assert_eq!(s.volume, 0.6);
        assert_eq!(s.preset, "custom");

        let s = Settings { focus_min: 50, short_min: 10, long_min: 20, ..Default::default() }.sanitized();
        assert_eq!(s.preset, "deep");
    }
}
