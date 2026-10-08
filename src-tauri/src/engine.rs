//! Glue between the pure timer, settings and the database. Tauri-free so the
//! whole session lifecycle can be tested headless.

use crate::db::{DayStats, Db};
use crate::settings::Settings;
use crate::timer::{EndStatus, Finished, Phase, RunState, Timer};
use chrono::{Duration, Local, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};

const K_SETTINGS: &str = "settings";
const K_TIMER: &str = "timer_state";

/// A user intent, sent from the UI as `{ type: "adjust", value: 300000 }`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum Action {
    Toggle,
    Reset,
    Restart,
    Skip,
    Adjust(i64),
    SetRemaining(i64),
    SwitchPhase(Phase),
    SetTask(Option<String>),
    Preset(String),
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub phase: Phase,
    pub run: RunState,
    pub duration_ms: i64,
    pub remaining_ms: i64,
    pub end_at: Option<i64>,
    pub cycle: u32,
    pub long_every: u32,
    pub task: Option<String>,
    pub preset: String,
    pub session_number: u32,
    pub completed_today: u32,
    pub focus_ms_today: i64,
    pub goal_ms: i64,
    pub hidden: bool,
    pub now: i64,
}

pub struct Engine {
    pub timer: Timer,
    pub settings: Settings,
    pub db: Db,
    pub hidden: bool,
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// [start, end) of the local calendar day containing `now`, DST-safe.
pub fn day_bounds(now: i64) -> (i64, i64) {
    let local = Local.timestamp_millis_opt(now).single().unwrap_or_else(Local::now);
    let midnight = |d: chrono::NaiveDate| {
        Local
            .from_local_datetime(&d.and_time(NaiveTime::MIN))
            .earliest()
            .map(|t| t.timestamp_millis())
            .unwrap_or(now)
    };
    let day = local.date_naive();
    (midnight(day), midnight(day + Duration::days(1)))
}

impl Engine {
    pub fn load(db: Db) -> Engine {
        let settings = db.kv_get::<Settings>(K_SETTINGS).unwrap_or_default().sanitized();
        let mut timer = db
            .kv_get::<Timer>(K_TIMER)
            .unwrap_or_else(|| Timer::new(settings.durations().focus_ms));
        timer.apply_durations(&settings.durations());
        let _ = db.close_orphans(timer.session_id);
        Engine { timer, settings, db, hidden: false }
    }

    pub fn today(&self, now: i64) -> DayStats {
        let (from, to) = day_bounds(now);
        self.db.day_stats(from, to).unwrap_or_default()
    }

    pub fn snapshot(&self, now: i64) -> Snapshot {
        let today = self.today(now);
        let t = &self.timer;
        Snapshot {
            phase: t.phase,
            run: t.run,
            duration_ms: t.duration_ms,
            remaining_ms: t.remaining(now),
            end_at: if t.run == RunState::Running { t.end_at } else { None },
            cycle: t.cycle,
            long_every: self.settings.long_every,
            task: t.task.clone(),
            preset: self.settings.preset.clone(),
            session_number: today.completed + u32::from(t.phase == Phase::Focus),
            completed_today: today.completed,
            focus_ms_today: today.focus_ms + if t.phase == Phase::Focus { t.active(now) } else { 0 },
            goal_ms: self.settings.daily_goal_min as i64 * 60_000,
            hidden: self.hidden,
            now,
        }
    }

    /// Apply a user action. Returns the block that ended as a result, already
    /// recorded in the database.
    pub fn apply(&mut self, action: Action, now: i64) -> Option<Finished> {
        let d = self.settings.durations();
        let finished = match action {
            Action::Toggle => {
                if self.timer.run == RunState::Running {
                    self.timer.pause(now);
                } else {
                    self.start(now);
                }
                None
            }
            Action::Reset => self.timer.reset(now, &d),
            Action::Restart => {
                let f = self.timer.reset(now, &d);
                self.record(f.as_ref());
                self.start(now);
                self.persist();
                return f;
            }
            Action::Skip => self.timer.skip(now, &d),
            Action::Adjust(delta) => self.timer.adjust(delta, now, &d),
            Action::SetRemaining(ms) => {
                self.timer.set_remaining(ms, now);
                None
            }
            Action::SwitchPhase(p) => self.timer.switch_phase(p, now, &d),
            Action::SetTask(task) => {
                let task = task.map(|s| s.trim().chars().take(120).collect::<String>()).filter(|s| !s.is_empty());
                if let Some(id) = self.timer.session_id {
                    let _ = self.db.set_session_task(id, task.as_deref());
                }
                self.timer.task = task;
                None
            }
            Action::Preset(id) => {
                if self.settings.apply_preset(&id) {
                    let _ = self.db.kv_set(K_SETTINGS, &self.settings);
                    self.timer.apply_durations(&self.settings.durations());
                }
                None
            }
        };
        self.after(finished, now)
    }

    /// Watchdog: completes the running block once its end time has passed.
    pub fn tick(&mut self, now: i64) -> Option<Finished> {
        let f = self.timer.tick(now, &self.settings.durations());
        if f.is_none() {
            return None;
        }
        self.after(f, now)
    }

    pub fn update_settings(&mut self, next: Settings) {
        self.settings = next.sanitized();
        let _ = self.db.kv_set(K_SETTINGS, &self.settings);
        self.timer.apply_durations(&self.settings.durations());
        self.persist();
    }

    fn after(&mut self, finished: Option<Finished>, now: i64) -> Option<Finished> {
        self.record(finished.as_ref());
        if let Some(f) = &finished {
            let auto = match self.timer.phase {
                Phase::Focus => self.settings.auto_start_focus,
                _ => self.settings.auto_start_breaks,
            };
            if f.status == EndStatus::Completed && auto {
                self.start(now);
            }
        }
        self.persist();
        finished
    }

    fn start(&mut self, now: i64) {
        let is_new = self.timer.run == RunState::Idle;
        if is_new && self.timer.phase == Phase::Focus && self.timer.cycle > 0 && self.today(now).completed == 0 {
            // First focus block of a new day starts a fresh cycle.
            self.timer.cycle = 0;
        }
        if self.timer.start(now) {
            self.timer.session_id = self
                .db
                .insert_session(
                    self.timer.phase,
                    now,
                    self.timer.duration_ms,
                    self.timer.task.as_deref(),
                    &self.settings.preset,
                )
                .ok();
        }
    }

    fn record(&self, f: Option<&Finished>) {
        if let Some(f) = f {
            let _ = self.db.finish_session(f, &self.settings.preset);
        }
    }

    fn persist(&self) {
        let _ = self.db.kv_set(K_TIMER, &self.timer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timer::MINUTE;

    fn engine() -> Engine {
        Engine::load(Db::open_in_memory().unwrap())
    }

    fn now() -> i64 {
        // Mid-morning today, so every block lands on the same local day.
        day_bounds(now_ms()).0 + 9 * 60 * MINUTE
    }

    #[test]
    fn full_focus_block_is_recorded_and_counted() {
        let mut e = engine();
        let t0 = now();
        e.apply(Action::SetTask(Some("  Finish AI PM resume  ".into())), t0);
        e.apply(Action::Toggle, t0);
        assert_eq!(e.snapshot(t0).session_number, 1);
        assert!(e.tick(t0 + 10 * MINUTE).is_none());

        let f = e.tick(t0 + 25 * MINUTE).expect("completed");
        assert_eq!(f.status, EndStatus::Completed);
        assert_eq!(f.task.as_deref(), Some("Finish AI PM resume"));

        let s = e.snapshot(t0 + 25 * MINUTE);
        assert_eq!(s.phase, Phase::ShortBreak);
        assert_eq!(s.run, RunState::Idle);
        assert_eq!(s.completed_today, 1);
        assert_eq!(s.focus_ms_today, 25 * MINUTE);
        let today = e.today(t0);
        assert_eq!(today.sessions.len(), 1);
        assert_eq!(today.sessions[0].task.as_deref(), Some("Finish AI PM resume"));
    }

    #[test]
    fn auto_start_breaks_chains_into_next_block() {
        let mut e = engine();
        e.update_settings(Settings { auto_start_breaks: true, ..e.settings.clone() });
        let t0 = now();
        e.apply(Action::Toggle, t0);
        e.tick(t0 + 25 * MINUTE).unwrap();
        assert_eq!(e.timer.phase, Phase::ShortBreak);
        assert_eq!(e.timer.run, RunState::Running);
        assert!(e.timer.session_id.is_some());
    }

    #[test]
    fn state_survives_restart() {
        let dir = std::env::temp_dir().join(format!("chronos-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.sqlite3");
        let _ = std::fs::remove_file(&path);
        let t0 = now();
        {
            let mut e = Engine::load(Db::open(&path).unwrap());
            e.apply(Action::Toggle, t0);
            e.apply(Action::Adjust(5 * MINUTE), t0 + MINUTE);
        }
        // "Crash", then relaunch long after the block should have ended.
        let mut e = Engine::load(Db::open(&path).unwrap());
        assert_eq!(e.timer.run, RunState::Running);
        let f = e.tick(t0 + 3 * 60 * MINUTE).expect("completed on relaunch");
        assert_eq!(f.ended_at, t0 + 30 * MINUTE);
        assert_eq!(e.today(t0).completed, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preset_applies_to_idle_timer_and_persists() {
        let mut e = engine();
        e.apply(Action::Preset("deep".into()), now());
        assert_eq!(e.timer.remaining_ms, 50 * MINUTE);
        assert_eq!(e.settings.preset, "deep");
        assert_eq!(e.db.kv_get::<Settings>(K_SETTINGS).unwrap().focus_min, 50);
    }

    #[test]
    fn restart_abandons_and_starts_fresh() {
        let mut e = engine();
        let t0 = now();
        e.apply(Action::Toggle, t0);
        let first = e.timer.session_id;
        let f = e.apply(Action::Restart, t0 + 5 * MINUTE).unwrap();
        assert_eq!(f.status, EndStatus::Abandoned);
        assert_eq!(e.timer.run, RunState::Running);
        assert_ne!(e.timer.session_id, first);
        assert_eq!(e.timer.remaining(t0 + 5 * MINUTE), 25 * MINUTE);
    }

    #[test]
    fn day_bounds_cover_a_day() {
        let (a, b) = day_bounds(now_ms());
        assert!(a <= now_ms() && now_ms() < b);
        assert!((23 * 60..=25 * 60).contains(&((b - a) / MINUTE)));
    }
}
