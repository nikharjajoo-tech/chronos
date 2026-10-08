//! Glue between the pure timer, settings and the database. Tauri-free so the
//! whole session lifecycle can be tested headless.

use crate::analytics::{self, Named, PeriodStats, Streak};
use crate::db::{DayStats, Db, DistractionRow};
use crate::settings::Settings;
use crate::timer::{EndStatus, Finished, Phase, RunState, Timer};
use chrono::{Duration, Local, NaiveDate, NaiveTime, TimeZone, Timelike};
use std::collections::BTreeSet;
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
    pub project: Option<String>,
    pub preset: String,
    pub session_number: u32,
    pub completed_today: u32,
    pub focus_ms_today: i64,
    pub goal_ms: i64,
    pub distractions_today: u32,
    /// Set when focus was auto-paused because you stepped away: how long.
    pub away_ms: Option<i64>,
    pub hidden: bool,
    pub now: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayStats {
    #[serde(flatten)]
    pub day: DayStats,
    pub streak: Streak,
    pub projects: Vec<Named>,
    pub distraction_list: Vec<DistractionRow>,
}

/// A logged distraction, for the quick popup.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoggedDistraction {
    pub id: i64,
    pub at: i64,
    pub count_today: u32,
    pub session_number: Option<u32>,
}

pub struct Engine {
    pub timer: Timer,
    pub settings: Settings,
    pub db: Db,
    pub hidden: bool,
    pub away_notice: Option<i64>,
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn local_date(t: i64) -> NaiveDate {
    Local.timestamp_millis_opt(t).single().unwrap_or_else(Local::now).date_naive()
}

/// Local midnight at the start of `d`, DST-safe.
pub fn local_midnight(d: NaiveDate) -> i64 {
    Local
        .from_local_datetime(&d.and_time(NaiveTime::MIN))
        .earliest()
        .map(|t| t.timestamp_millis())
        .unwrap_or_else(|| d.and_time(NaiveTime::MIN).and_utc().timestamp_millis())
}

fn local_hour(t: i64) -> usize {
    Local.timestamp_millis_opt(t).single().map(|d| d.hour() as usize).unwrap_or(0)
}

/// [start, end) of the local calendar day containing `now`.
pub fn day_bounds(now: i64) -> (i64, i64) {
    let day = local_date(now);
    (local_midnight(day), local_midnight(day + Duration::days(1)))
}

fn days_between(first: NaiveDate, next: NaiveDate) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    let mut d = first;
    while d < next {
        out.push((local_midnight(d), local_midnight(d + Duration::days(1))));
        d += Duration::days(1);
    }
    out
}

/// "Finish resume #job-search" → (Some("Finish resume"), Some("job-search")).
/// The first #tag is the project; tags are lower-cased so #CV and #cv match.
pub fn split_project(text: &str) -> (Option<String>, Option<String>) {
    let mut project: Option<String> = None;
    let mut words = Vec::new();
    for w in text.split_whitespace() {
        match w.strip_prefix('#') {
            Some(tag) => {
                let tag: String = tag
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
                    .take(32)
                    .collect::<String>()
                    .to_lowercase();
                if project.is_none() && !tag.is_empty() {
                    project = Some(tag);
                }
            }
            None => words.push(w),
        }
    }
    let task: String = words.join(" ").chars().take(120).collect();
    (Some(task).filter(|t| !t.is_empty()), project)
}

impl Engine {
    pub fn load(db: Db) -> Engine {
        let settings = db.kv_get::<Settings>(K_SETTINGS).unwrap_or_default().sanitized();
        let mut timer = db
            .kv_get::<Timer>(K_TIMER)
            .unwrap_or_else(|| Timer::new(settings.durations().focus_ms));
        timer.apply_durations(&settings.durations());
        let _ = db.close_orphans(timer.session_id);
        Engine { timer, settings, db, hidden: false, away_notice: None }
    }

    pub fn today(&self, now: i64) -> DayStats {
        let (from, to) = day_bounds(now);
        self.db.day_stats(from, to).unwrap_or_default()
    }

    pub fn today_full(&self, now: i64) -> TodayStats {
        let (from, to) = day_bounds(now);
        let day = self.today(now);
        let distraction_list = self.db.distractions_between(from, to).unwrap_or_default();
        let (projects, ..) = analytics::breakdowns(&day.sessions, &[], local_hour);
        TodayStats { streak: self.streak(now), projects, distraction_list, day }
    }

    pub fn streak(&self, now: i64) -> Streak {
        let active: BTreeSet<NaiveDate> =
            self.db.completed_focus_starts(0).unwrap_or_default().into_iter().map(local_date).collect();
        analytics::streak(&active, local_date(now))
    }

    /// Week or month `offset` periods back (0 = current), compared with the
    /// period before it. For the current period the comparison only covers
    /// the same number of days, so a Thursday isn't measured against a full week.
    pub fn period(&self, kind: &str, offset: i32, now: i64) -> PeriodStats {
        let today = local_date(now);
        let (first, next) = analytics::period_dates(kind, offset.min(0), today);
        let days = days_between(first, next);
        let (from, to) = (days[0].0, days[days.len() - 1].1);
        let sessions = self.db.sessions_between(from, to).unwrap_or_default();
        let distractions = self.db.distractions_between(from, to).unwrap_or_default();
        let (agg, totals) = analytics::totals(&days, now, &sessions, &distractions);

        let (pfirst, pnext) = analytics::period_dates(kind, offset.min(0) - 1, today);
        let mut pdays = days_between(pfirst, pnext);
        pdays.truncate(totals.elapsed_days.max(1) as usize);
        let (pfrom, pto) = (pdays[0].0, pdays[pdays.len() - 1].1);
        let (_, previous) = analytics::totals(
            &pdays,
            now,
            &self.db.sessions_between(pfrom, pto).unwrap_or_default(),
            &self.db.distractions_between(pfrom, pto).unwrap_or_default(),
        );

        let (projects, categories, focus_by_hour, distractions_by_hour) =
            analytics::breakdowns(&sessions, &distractions, local_hour);
        PeriodStats {
            from,
            to,
            days: agg,
            totals,
            previous,
            projects,
            categories,
            focus_by_hour,
            distractions_by_hour,
            streak: self.streak(now),
        }
    }

    /// Record a distraction right now, tied to the focus session if one is on.
    pub fn log_distraction(&mut self, now: i64) -> Option<LoggedDistraction> {
        let in_focus = self.timer.phase == Phase::Focus && self.timer.run != RunState::Idle;
        let session_id = if in_focus { self.timer.session_id } else { None };
        let id = self.db.insert_distraction(now, session_id).ok()?;
        let today = self.today(now);
        Some(LoggedDistraction {
            id,
            at: now,
            count_today: today.distractions,
            session_number: in_focus.then_some(today.completed + 1),
        })
    }

    /// Nobody has touched the keyboard or mouse since `since`. Pauses a
    /// running focus block as of that moment; returns true if it did.
    pub fn away(&mut self, since: i64, now: i64) -> bool {
        match self.timer.pause_away(since, now) {
            Some(ms) => {
                self.away_notice = Some(ms);
                self.persist();
                true
            }
            None => false,
        }
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
            project: t.project.clone(),
            preset: self.settings.preset.clone(),
            session_number: today.completed + u32::from(t.phase == Phase::Focus),
            completed_today: today.completed,
            focus_ms_today: today.focus_ms + if t.phase == Phase::Focus { t.active(now) } else { 0 },
            goal_ms: self.settings.daily_goal_min as i64 * 60_000,
            distractions_today: today.distractions,
            away_ms: self.away_notice,
            hidden: self.hidden,
            now,
        }
    }

    /// Apply a user action. Returns the block that ended as a result, already
    /// recorded in the database.
    pub fn apply(&mut self, action: Action, now: i64) -> Option<Finished> {
        let d = self.settings.durations();
        self.away_notice = None;
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
            Action::SetTask(text) => {
                let (task, project) = split_project(text.as_deref().unwrap_or(""));
                if let Some(id) = self.timer.session_id {
                    let _ = self.db.set_session_task(id, task.as_deref(), project.as_deref());
                }
                self.timer.task = task;
                self.timer.project = project;
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
                    self.timer.project.as_deref(),
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
    fn project_tags_are_split_from_the_task() {
        assert_eq!(split_project("Finish AI PM resume #Job-Search"), (Some("Finish AI PM resume".into()), Some("job-search".into())));
        assert_eq!(split_project("#cv"), (None, Some("cv".into())));
        assert_eq!(split_project("Write #blog post #ignored"), (Some("Write post".into()), Some("blog".into())));
        assert_eq!(split_project("  plain task "), (Some("plain task".into()), None));
        assert_eq!(split_project("# lonely hash"), (Some("lonely hash".into()), None));
    }

    #[test]
    fn project_is_recorded_with_the_session() {
        let mut e = engine();
        let t0 = now();
        e.apply(Action::SetTask(Some("Draft case study #portfolio".into())), t0);
        e.apply(Action::Toggle, t0);
        e.tick(t0 + 25 * MINUTE).unwrap();
        let today = e.today_full(t0);
        assert_eq!(today.day.sessions[0].project.as_deref(), Some("portfolio"));
        assert_eq!(today.projects[0].name, "portfolio");
        assert_eq!(e.snapshot(t0).project.as_deref(), Some("portfolio"), "project carries into the break");
    }

    #[test]
    fn away_pause_shows_notice_until_next_action() {
        let mut e = engine();
        let t0 = now();
        e.apply(Action::Toggle, t0);
        assert!(e.away(t0 + 8 * MINUTE, t0 + 13 * MINUTE));
        let s = e.snapshot(t0 + 13 * MINUTE);
        assert_eq!(s.run, RunState::Paused);
        assert_eq!(s.away_ms, Some(5 * MINUTE));
        assert_eq!(s.remaining_ms, 17 * MINUTE);
        e.apply(Action::Toggle, t0 + 14 * MINUTE);
        assert_eq!(e.snapshot(t0 + 14 * MINUTE).away_ms, None);
        assert!(!e.away(t0, t0 + 15 * MINUTE) || e.timer.run == RunState::Paused);
    }

    #[test]
    fn distractions_link_to_the_running_focus_session() {
        let mut e = engine();
        let t0 = now();
        let idle = e.log_distraction(t0).unwrap();
        assert_eq!(idle.session_number, None);
        e.apply(Action::Toggle, t0);
        let d = e.log_distraction(t0 + MINUTE).unwrap();
        assert_eq!(d.count_today, 2);
        assert_eq!(d.session_number, Some(1));
        let rows = e.db.distractions_between(t0, t0 + 2 * MINUTE).unwrap();
        assert_eq!(rows[1].session_id, e.timer.session_id);
        assert_eq!(e.snapshot(t0 + MINUTE).distractions_today, 2);
    }

    #[test]
    fn period_stats_cover_the_current_week() {
        let mut e = engine();
        let t0 = now();
        e.apply(Action::Toggle, t0);
        e.tick(t0 + 25 * MINUTE).unwrap();
        let w = e.period("week", 0, t0 + 30 * MINUTE);
        assert_eq!(w.days.len(), 7);
        assert_eq!(w.totals.completed, 1);
        assert_eq!(w.streak.current, 1);
        assert!(w.streak.today_done);
        let m = e.period("month", 0, t0 + 30 * MINUTE);
        assert!((28..=31).contains(&m.days.len()));
        assert_eq!(m.totals.focus_ms, 25 * MINUTE);
    }

    #[test]
    fn day_bounds_cover_a_day() {
        let (a, b) = day_bounds(now_ms());
        assert!(a <= now_ms() && now_ms() < b);
        assert!((23 * 60..=25 * 60).contains(&((b - a) / MINUTE)));
    }
}
