//! Glue between the pure timer, settings and the database. Tauri-free so the
//! whole session lifecycle can be tested headless.

use crate::analytics::{self, Named, PeriodStats, Streak};
use crate::insights::{self, Insight, Score, Suggestion};
use crate::report::{self, Email, ReportData};
use crate::db::{DayStats, Db, DistractionRow};
use crate::settings::Settings;
use crate::tasks::{self, Bucket, TaskRow};
use crate::timer::{EndStatus, Finished, Phase, RunState, Timer};
use chrono::{Datelike, Duration, Local, NaiveDate, NaiveTime, TimeZone, Timelike};
use std::collections::BTreeSet;
use serde::{Deserialize, Serialize};

const K_SETTINGS: &str = "settings";
const K_TIMER: &str = "timer_state";
const K_DISMISSED: &str = "suggestions_dismissed";
/// At most one companion line per this long (the goal line is exempt).
const COMPANION_GAP_MS: i64 = 90 * 60_000;

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
    /// Typed on the clock: an open Today task with that title goes live, or a
    /// new one is added to the top of Today. Empty = free focus.
    SetTask(Option<String>),
    Preset(String),
    /// Make a task live (moving it to the top of Today if needed) and focus on it.
    StartTask(i64),
    /// Tick off the live task; the next Today task goes live, the timer keeps going.
    DoneTask,
    /// "Not now": the live task goes to the bottom of Today, the next one goes live.
    NextTask,
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
    pub task_id: Option<i64>,
    pub task_estimate_ms: Option<i64>,
    /// All focus time on the live task, as of `now`.
    pub task_spent_ms: i64,
    /// Open tasks in Today.
    pub today_open: u32,
    pub preset: String,
    pub session_number: u32,
    pub completed_today: u32,
    pub focus_ms_today: i64,
    pub goal_ms: i64,
    pub distractions_today: u32,
    /// Set when focus was auto-paused because you stepped away: how long.
    pub away_ms: Option<i64>,
    /// A rare one-line nudge shown after a session ends.
    pub companion: Option<String>,
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
    pub week_score: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeekScore {
    pub start: i64,
    pub value: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightsView {
    /// This week so far.
    pub score: Option<Score>,
    /// The same days of last week.
    pub previous: Option<Score>,
    /// The part that moved the score most: (label, ±points).
    pub mover: Option<(String, f64)>,
    /// Oldest first, the last entry is this week.
    pub trend: Vec<WeekScore>,
    pub insights: Vec<Insight>,
    pub suggestion: Option<Suggestion>,
    /// Focus sessions the insights are based on (last 28 days).
    pub sample: usize,
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
    companion: CompanionMemory,
}

/// What the companion remembers between sessions (in memory only).
#[derive(Default)]
struct CompanionMemory {
    /// Focus time since the last real break.
    focus_run_ms: i64,
    /// Breaks skipped in a row.
    skipped_breaks: u32,
    last_focus_end: Option<i64>,
    line: Option<String>,
    last_line_at: Option<i64>,
    goal_announced: Option<NaiveDate>,
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
    let (words, project) = tasks::split_tags(text);
    let task: String = words.join(" ").chars().take(tasks::MAX_TITLE).collect();
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
        Engine { timer, settings, db, hidden: false, away_notice: None, companion: CompanionMemory::default() }
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
        let week_score = self.week_score(0, None, now).map(|s| s.value);
        TodayStats { streak: self.streak(now), projects, distraction_list, day, week_score }
    }

    /// Score for the week `offset` weeks back; `days` truncates it (to compare
    /// like with like against a week in progress).
    fn week_score(&self, offset: i32, days: Option<usize>, now: i64) -> Option<Score> {
        let (first, next) = analytics::period_dates("week", offset, local_date(now));
        let mut days_v = days_between(first, next);
        if let Some(n) = days {
            days_v.truncate(n.max(1));
        }
        let (from, to) = (days_v[0].0, days_v[days_v.len() - 1].1);
        let sessions = self.db.sessions_between(from, to).ok()?;
        let distractions = self.db.distractions_between(from, to).ok()?;
        let (_, totals) = analytics::totals(&days_v, now, &sessions, &distractions);
        insights::score(&totals, &sessions, self.settings.daily_goal_min as i64 * 60_000)
    }

    pub fn insights_view(&self, now: i64) -> InsightsView {
        let score = self.week_score(0, None, now);
        let elapsed = (local_date(now).weekday().num_days_from_monday() + 1) as usize;
        let previous = self.week_score(-1, Some(elapsed), now);
        let mover = match (&score, &previous) {
            (Some(a), Some(b)) => insights::biggest_mover(a, b).map(|(l, d)| (l.to_string(), d)),
            _ => None,
        };
        let trend = (-7..=0)
            .map(|o| WeekScore {
                start: local_midnight(analytics::period_dates("week", o, local_date(now)).0),
                value: self.week_score(o, None, now).map(|s| s.value),
            })
            .collect();

        const DAY: i64 = 24 * 60 * 60_000;
        let recent = self.db.sessions_between(now - 28 * DAY, now + 1).unwrap_or_default();
        let distractions = self.db.distractions_between(now - 28 * DAY, now + 1).unwrap_or_default();
        let clock = insights::Clock {
            hour: &local_hour,
            weekday: &|t| local_date(t).weekday().num_days_from_monday() as usize,
            day: &|t| local_date(t).num_days_from_ce() as i64,
        };
        let found = insights::insights(&recent, &distractions, now, &clock);

        let week: Vec<_> = recent.iter().filter(|s| s.started_at >= now - 7 * DAY).cloned().collect();
        let prev: Vec<_> = recent.iter().filter(|s| s.started_at >= now - 14 * DAY && s.started_at < now - 7 * DAY).cloned().collect();
        let d_week = distractions.iter().filter(|d| d.at >= now - 7 * DAY).count();
        let d_prev = distractions.iter().filter(|d| d.at >= now - 14 * DAY && d.at < now - 7 * DAY).count();
        let dismissed = self.db.kv_get::<Vec<String>>(K_DISMISSED).unwrap_or_default();
        let suggestion = insights::suggestion(&self.settings, &week, &prev, d_week, d_prev, &found, &dismissed);

        InsightsView {
            score,
            previous,
            mover,
            trend,
            insights: found,
            suggestion,
            sample: recent.iter().filter(|s| s.is_focus()).count(),
        }
    }

    /// Everything for the weekly email about the week `offset` weeks back.
    pub fn report_data(&self, offset: i32, now: i64) -> ReportData {
        let score = self.week_score(offset, None, now);
        let prev_score = self.week_score(offset - 1, None, now);
        let mover = match (&score, &prev_score) {
            (Some(a), Some(b)) => insights::biggest_mover(a, b).map(|(l, d)| (l.to_string(), d)),
            _ => None,
        };
        let view = self.insights_view(now);
        ReportData {
            period: self.period("week", offset, now),
            score,
            prev_score,
            mover,
            insights: view.insights,
            suggestion: view.suggestion,
            goal_ms: self.settings.daily_goal_min as i64 * 60_000,
        }
    }

    pub fn render_report(&self, offset: i32, now: i64) -> (Email, bool) {
        let data = self.report_data(offset, now);
        let empty = data.period.totals.focus_sessions == 0;
        let fmt = |t: i64, f: &str| Local.timestamp_millis_opt(t).single().map(|d| d.format(f).to_string()).unwrap_or_default();
        (report::render(&data, &fmt), empty)
    }

    /// Accept or decline the current suggestion. Either way it never comes back.
    /// Returns true if settings changed.
    pub fn respond_suggestion(&mut self, id: &str, accept: bool, now: i64) -> bool {
        let current = self.insights_view(now).suggestion.filter(|s| s.id == id);
        let mut dismissed = self.db.kv_get::<Vec<String>>(K_DISMISSED).unwrap_or_default();
        if !dismissed.iter().any(|d| d == id) {
            dismissed.push(id.to_string());
            let _ = self.db.kv_set(K_DISMISSED, &dismissed);
        }
        let Some(action) = current.and_then(|s| s.action).filter(|_| accept) else { return false };
        let mut next = self.settings.clone();
        match action.setting {
            "focusMin" => next.focus_min = action.value.as_u64().unwrap_or(next.focus_min as u64) as u32,
            "dnd" => next.dnd = true,
            "autoStartBreaks" => next.auto_start_breaks = true,
            _ => return false,
        }
        self.update_settings(next);
        true
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
        let live = t.task_id.and_then(|id| self.db.task(id, day_bounds(now).0).ok().flatten());
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
            task_id: live.as_ref().map(|r| r.id),
            task_estimate_ms: live.as_ref().and_then(|r| r.estimate_ms),
            task_spent_ms: live.as_ref().map_or(0, |r| r.spent_ms + t.task_credit(r.id, now)),
            today_open: self.db.open_count(Bucket::Today).unwrap_or(0),
            preset: self.settings.preset.clone(),
            session_number: today.completed + u32::from(t.phase == Phase::Focus),
            completed_today: today.completed,
            focus_ms_today: today.focus_ms + if t.phase == Phase::Focus { t.active(now) } else { 0 },
            goal_ms: self.settings.daily_goal_min as i64 * 60_000,
            distractions_today: today.distractions,
            away_ms: self.away_notice,
            companion: self.companion.line.clone(),
            hidden: self.hidden,
            now,
        }
    }

    /// Apply a user action. Returns the block that ended as a result, already
    /// recorded in the database.
    pub fn apply(&mut self, action: Action, now: i64) -> Option<Finished> {
        let d = self.settings.durations();
        self.away_notice = None;
        self.companion.line = None;
        // Skipping a break you never started is the most common way to skip one.
        let leaving_break = matches!(action, Action::Skip | Action::SwitchPhase(Phase::Focus) | Action::StartTask(_));
        if leaving_break && self.timer.phase != Phase::Focus && self.timer.run == RunState::Idle {
            self.companion.skipped_breaks += 1;
        }
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
                let mut f = self.timer.reset(now, &d);
                self.record(&mut f);
                if let Some(f) = &f {
                    self.remember(f, now);
                }
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
                let text = text.unwrap_or_default();
                match tasks::parse(&text) {
                    Some(parsed) => {
                        let today = self.db.open_tasks(Bucket::Today, day_bounds(now).0).unwrap_or_default();
                        let wanted = parsed.title.to_lowercase();
                        let row = match today.into_iter().find(|r| r.title.to_lowercase() == wanted) {
                            Some(row) => Some(row),
                            None => self.db.insert_task(&parsed, Bucket::Today, true, now).ok().and_then(|id| self.task_row(id, now)),
                        };
                        self.go_live(row.as_ref(), now);
                    }
                    None => {
                        // Nothing but a #tag (or nothing): free focus.
                        self.go_live(None, now);
                        self.timer.project = tasks::split_tags(&text).1;
                    }
                }
                None
            }
            Action::Preset(id) => {
                if self.settings.apply_preset(&id) {
                    let _ = self.db.kv_set(K_SETTINGS, &self.settings);
                    self.timer.apply_durations(&self.settings.durations());
                }
                None
            }
            Action::StartTask(id) => {
                let Some(row) = self.task_row(id, now) else { return self.after(None, now) };
                if row.done {
                    let _ = self.db.set_task_done(id, false, now);
                }
                if row.bucket != Bucket::Today {
                    let first = self.top_today(now).map(|r| r.id);
                    let _ = self.db.move_task(id, Bucket::Today, first, now);
                }
                let mut f = None;
                if self.timer.phase != Phase::Focus {
                    f = self.timer.switch_phase(Phase::Focus, now, &d);
                    self.record(&mut f);
                    if let Some(f) = &f {
                        self.remember(f, now);
                    }
                }
                let row = self.task_row(id, now);
                self.go_live(row.as_ref(), now);
                if self.timer.run != RunState::Running {
                    self.start(now);
                }
                self.persist();
                return f;
            }
            Action::DoneTask => {
                if let Some(id) = self.timer.task_id {
                    let _ = self.db.set_task_done(id, true, now);
                    self.reconcile_live(now);
                }
                None
            }
            Action::NextTask => {
                if let Some(id) = self.timer.task_id {
                    let _ = self.db.move_task(id, Bucket::Today, None, now);
                    let next = self.top_today(now);
                    self.go_live(next.as_ref(), now);
                }
                None
            }
        };
        self.after(finished, now)
    }

    /// Open tasks plus tasks done in the last `done_days` local days, with
    /// the running block's credit included.
    pub fn tasks_list(&self, done_days: u32, now: i64) -> Vec<TaskRow> {
        let today = local_date(now);
        let done_since = match done_days {
            0 => i64::MAX,
            n => local_midnight(today - Duration::days(n as i64 - 1)),
        };
        let mut rows = self.db.tasks_list(done_since, local_midnight(today)).unwrap_or_default();
        for r in &mut rows {
            r.spent_ms += self.timer.task_credit(r.id, now);
        }
        rows
    }

    /// Call after any change to the task list. Keeps the live task in step:
    /// a deleted one loses this block's time, one that was ticked off or moved
    /// out of Today is credited, and either way the next Today task takes over.
    /// An edited title or #tag shows on the clock straight away.
    pub fn reconcile_live(&mut self, now: i64) {
        let Some(id) = self.timer.task_id else { return };
        match self.db.task(id, day_bounds(now).0) {
            Ok(Some(row)) if !row.done && row.bucket == Bucket::Today => self.go_live(Some(&row), now),
            Ok(row) => {
                if row.is_none() {
                    self.timer.drop_task(id, now);
                }
                let next = self.top_today(now);
                self.go_live(next.as_ref(), now);
            }
            Err(_) => {}
        }
        self.persist();
    }

    fn task_row(&self, id: i64, now: i64) -> Option<TaskRow> {
        self.db.task(id, day_bounds(now).0).ok().flatten()
    }

    fn top_today(&self, now: i64) -> Option<TaskRow> {
        self.db.open_tasks(Bucket::Today, day_bounds(now).0).ok()?.into_iter().next()
    }

    /// Makes `row` the live task (None = free focus), crediting the outgoing
    /// one, and puts its title and #tag on the clock.
    fn go_live(&mut self, row: Option<&TaskRow>, now: i64) {
        self.timer.set_task(row.map(|r| r.id), now);
        self.timer.task = row.map(|r| r.title.clone());
        self.timer.project = row.and_then(|r| r.project.clone());
        if let Some(id) = self.timer.session_id.filter(|_| self.timer.is_started()) {
            let _ = self.db.set_session_task(id, self.timer.task.as_deref(), self.timer.project.as_deref());
        }
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

    fn after(&mut self, mut finished: Option<Finished>, now: i64) -> Option<Finished> {
        self.record(&mut finished);
        if let Some(f) = &finished {
            self.remember(f, now);
        }
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

    /// Update the companion's memory with a block that just ended, and maybe
    /// pick a line to say. Rare by design.
    fn remember(&mut self, f: &Finished, now: i64) {
        let c = &mut self.companion;
        if f.phase == Phase::Focus {
            if f.status != EndStatus::Cancelled {
                c.focus_run_ms += f.actual_ms;
                c.last_focus_end = Some(f.ended_at);
            }
        } else if f.status == EndStatus::Completed || f.actual_ms >= 3 * 60_000 {
            c.focus_run_ms = 0;
            c.skipped_breaks = 0;
        } else {
            c.skipped_breaks += 1;
        }
        if f.phase != Phase::Focus || f.status != EndStatus::Completed || !self.settings.companion {
            return;
        }
        let goal = self.settings.daily_goal_min as i64 * 60_000;
        let today = local_date(f.ended_at);
        let focused = self.today(now).focus_ms;
        let goal_hit = (goal > 0 && focused >= goal && focused - f.actual_ms < goal && self.companion.goal_announced != Some(today))
            .then_some(goal);
        let c = &mut self.companion;
        if let Some((is_goal, line)) = insights::companion_line(c.focus_run_ms, c.skipped_breaks, goal_hit) {
            let quiet = c.last_line_at.is_some_and(|t| now - t < COMPANION_GAP_MS);
            if is_goal || !quiet {
                c.line = Some(line);
                c.last_line_at = Some(now);
                if is_goal {
                    c.goal_announced = Some(today);
                }
            }
        }
    }

    fn start(&mut self, now: i64) {
        let is_new = self.timer.run == RunState::Idle;
        // Away from the clock for a while counts as a real break.
        if is_new && self.timer.phase == Phase::Focus && self.companion.last_focus_end.is_some_and(|e| now - e >= 15 * 60_000) {
            self.companion.focus_run_ms = 0;
            self.companion.skipped_breaks = 0;
        }
        if is_new && self.timer.phase == Phase::Focus && self.timer.cycle > 0 && self.today(now).completed == 0 {
            // First focus block of a new day starts a fresh cycle.
            self.timer.cycle = 0;
        }
        if is_new && self.timer.phase == Phase::Focus {
            // A new focus block works on the top Today task unless the live
            // one is still open in Today. Text typed before the planner existed
            // stays when Today is empty.
            match self.timer.task_id.and_then(|id| self.task_row(id, now)).filter(|r| !r.done && r.bucket == Bucket::Today) {
                Some(row) => self.go_live(Some(&row), now),
                None => match self.top_today(now) {
                    Some(top) => self.go_live(Some(&top), now),
                    None if self.timer.task_id.is_some() => self.go_live(None, now),
                    None => {}
                },
            }
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

    /// Writes a block that ended. A block that covered several tasks is
    /// named after the one with the most time (the later one on a tie).
    fn record(&self, f: &mut Option<Finished>) {
        let Some(f) = f else { return };
        if let Some(&(id, _)) = f.task_ms.iter().max_by_key(|(_, ms)| *ms) {
            if let Some(row) = self.task_row(id, f.ended_at) {
                f.task_id = Some(id);
                f.task = Some(row.title);
                f.project = row.project;
            }
        }
        let _ = self.db.finish_session(f, &self.settings.preset);
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
    fn companion_speaks_after_a_long_unbroken_run_then_stays_quiet() {
        let mut e = engine();
        e.update_settings(Settings { daily_goal_min: 0, ..e.settings.clone() });
        let mut t = now();
        let mut lines = Vec::new();
        for _ in 0..6 {
            e.apply(Action::Toggle, t);
            t += 25 * MINUTE;
            e.tick(t).unwrap();
            lines.push(e.snapshot(t).companion);
            // Skip the break straight away (counts as a skipped break).
            e.apply(Action::Skip, t);
        }
        // Run of 50 min with 1 skipped break → nothing; 75 min with 2 skipped → "pushing".
        assert_eq!(lines[0], None);
        assert!(lines[2].as_deref().unwrap_or("").contains("Pushing"), "{lines:?}");
        // Then quiet for 90 minutes even though the run keeps growing.
        assert_eq!(lines[3], None);
        assert_eq!(lines[4], None);
    }

    #[test]
    fn companion_announces_the_daily_goal_once() {
        let mut e = engine();
        e.update_settings(Settings { daily_goal_min: 50, ..e.settings.clone() });
        let mut t = now();
        let mut lines = Vec::new();
        for _ in 0..3 {
            e.apply(Action::Toggle, t);
            t += 25 * MINUTE;
            e.tick(t).unwrap();
            lines.push(e.snapshot(t).companion);
            e.apply(Action::Toggle, t); // take the break
            t += 5 * MINUTE;
            e.tick(t).unwrap();
        }
        assert_eq!(lines[0], None);
        assert_eq!(lines[1].as_deref(), Some("Daily goal hit: 50m. Nice work."));
        assert_eq!(lines[2], None);
    }

    #[test]
    fn insights_view_and_suggestion_round_trip() {
        let mut e = engine();
        let mut t = now() - 5 * 60 * MINUTE;
        for _ in 0..8 {
            e.apply(Action::Toggle, t);
            t += 25 * MINUTE;
            e.tick(t).unwrap();
            e.apply(Action::Toggle, t); // take the break (short or long)
            t += e.timer.duration_ms;
            e.tick(t).unwrap();
        }
        let v = e.insights_view(t);
        assert!(v.score.is_some());
        assert_eq!(v.trend.len(), 8);
        assert_eq!(v.sample, 8);
        let s = v.suggestion.expect("suggestion");
        assert_eq!(s.id, "focus:25->30");
        assert!(e.respond_suggestion(&s.id, true, t));
        assert_eq!(e.settings.focus_min, 30);
        assert!(e.insights_view(t).suggestion.map_or(true, |x| x.id != s.id), "never repeated");
    }

    fn add_task(e: &Engine, text: &str, bucket: Bucket, t: i64) -> i64 {
        e.db.insert_task(&tasks::parse(text).unwrap(), bucket, false, t).unwrap()
    }

    fn spent(e: &Engine, id: i64, t: i64) -> i64 {
        e.tasks_list(1, t).into_iter().find(|r| r.id == id).map_or(-1, |r| r.spent_ms)
    }

    fn today_titles(e: &Engine, t: i64) -> Vec<String> {
        e.db.open_tasks(Bucket::Today, day_bounds(t).0).unwrap().into_iter().map(|r| r.title).collect()
    }

    #[test]
    fn live_task_top_today_goes_live_on_start() {
        let mut e = engine();
        let t0 = now();
        let a = add_task(&e, "Prep slides 28m #job-search", Bucket::Today, t0);
        add_task(&e, "Tailor resume", Bucket::Today, t0);
        assert_eq!(e.snapshot(t0).task_id, None, "nothing is live until focus starts");
        assert_eq!(e.snapshot(t0).today_open, 2);
        e.apply(Action::Toggle, t0);
        let s = e.snapshot(t0 + 10 * MINUTE);
        assert_eq!((s.task_id, s.task.as_deref(), s.project.as_deref()), (Some(a), Some("Prep slides"), Some("job-search")));
        assert_eq!((s.task_estimate_ms, s.task_spent_ms), (Some(28 * MINUTE), 10 * MINUTE));
    }

    #[test]
    fn live_task_reordering_does_not_switch_it() {
        let mut e = engine();
        let t0 = now();
        let a = add_task(&e, "a", Bucket::Today, t0);
        let b = add_task(&e, "b", Bucket::Today, t0);
        e.apply(Action::Toggle, t0);
        e.db.move_task(b, Bucket::Today, Some(a), t0).unwrap();
        e.reconcile_live(t0 + MINUTE);
        assert_eq!(e.timer.task_id, Some(a));
    }

    #[test]
    fn live_task_start_task_from_backlog_moves_to_top_of_today() {
        let mut e = engine();
        let t0 = now();
        add_task(&e, "a", Bucket::Today, t0);
        let z = add_task(&e, "z", Bucket::Backlog, t0);
        e.apply(Action::StartTask(z), t0);
        assert_eq!(today_titles(&e, t0), ["z", "a"]);
        assert_eq!((e.timer.task_id, e.timer.run), (Some(z), RunState::Running));
    }

    #[test]
    fn live_task_start_task_during_break_switches_to_focus() {
        let mut e = engine();
        let t0 = now();
        let a = add_task(&e, "a", Bucket::Today, t0);
        let b = add_task(&e, "b", Bucket::Today, t0);
        e.apply(Action::Toggle, t0);
        e.tick(t0 + 25 * MINUTE).unwrap();
        assert_eq!((e.timer.phase, e.timer.task_id), (Phase::ShortBreak, Some(a)), "a is up next");
        e.apply(Action::Toggle, t0 + 25 * MINUTE);
        let f = e.apply(Action::StartTask(b), t0 + 27 * MINUTE).expect("the break ended");
        assert_eq!(f.phase, Phase::ShortBreak);
        assert_eq!((e.timer.phase, e.timer.run, e.timer.task_id), (Phase::Focus, RunState::Running, Some(b)));
        assert_eq!(spent(&e, a, t0 + 30 * MINUTE), 25 * MINUTE, "no credit during the break");
        assert_eq!(spent(&e, b, t0 + 30 * MINUTE), 3 * MINUTE);
    }

    #[test]
    fn task_credit_is_split_at_a_switch_and_names_the_session() {
        let mut e = engine();
        let t0 = now();
        let a = add_task(&e, "a #one", Bucket::Today, t0);
        let b = add_task(&e, "b #two", Bucket::Today, t0);
        e.apply(Action::Toggle, t0);
        e.apply(Action::StartTask(b), t0 + 10 * MINUTE);
        let f = e.tick(t0 + 25 * MINUTE).unwrap();
        assert_eq!(f.task_ms, vec![(a, 10 * MINUTE), (b, 15 * MINUTE)]);
        assert_eq!((spent(&e, a, t0), spent(&e, b, t0)), (10 * MINUTE, 15 * MINUTE));
        let row = &e.today(t0).sessions[0];
        assert_eq!((row.task.as_deref(), row.project.as_deref()), (Some("b"), Some("two")), "most time wins");
        assert_eq!(e.today_full(t0).projects[0].name, "two");
    }

    #[test]
    fn task_credit_skips_pauses_and_cancelled_blocks_but_keeps_abandoned_ones() {
        let mut e = engine();
        let t0 = now();
        let a = add_task(&e, "a", Bucket::Today, t0);
        e.apply(Action::Toggle, t0);
        e.apply(Action::Reset, t0 + 30_000);
        assert_eq!(spent(&e, a, t0 + MINUTE), 0, "cancelled");

        e.apply(Action::Toggle, t0 + MINUTE);
        e.apply(Action::Toggle, t0 + 5 * MINUTE);
        e.apply(Action::Toggle, t0 + 20 * MINUTE);
        assert!(e.away(t0 + 22 * MINUTE, t0 + 30 * MINUTE));
        assert_eq!(e.snapshot(t0 + 40 * MINUTE).task_spent_ms, 6 * MINUTE);
        e.apply(Action::Reset, t0 + 40 * MINUTE);
        assert_eq!(spent(&e, a, t0 + 41 * MINUTE), 6 * MINUTE, "abandoned keeps its credit");
    }

    #[test]
    fn done_task_keeps_timer_running() {
        let mut e = engine();
        let t0 = now();
        let a = add_task(&e, "a", Bucket::Today, t0);
        let b = add_task(&e, "b", Bucket::Today, t0);
        e.apply(Action::Toggle, t0);
        assert!(e.apply(Action::DoneTask, t0 + 8 * MINUTE).is_none());
        assert_eq!((e.timer.run, e.timer.task_id), (RunState::Running, Some(b)));
        assert!(e.task_row(a, t0).unwrap().done);
        assert_eq!(spent(&e, a, t0 + 9 * MINUTE), 8 * MINUTE);
        e.apply(Action::DoneTask, t0 + 9 * MINUTE);
        let s = e.snapshot(t0 + 10 * MINUTE);
        assert_eq!((s.run, s.task_id, s.task, s.today_open), (RunState::Running, None, None, 0), "free focus");
    }

    #[test]
    fn next_task_moves_to_bottom() {
        let mut e = engine();
        let t0 = now();
        add_task(&e, "a", Bucket::Today, t0);
        let b = add_task(&e, "b", Bucket::Today, t0);
        add_task(&e, "c", Bucket::Today, t0);
        e.apply(Action::Toggle, t0);
        e.apply(Action::NextTask, t0 + MINUTE);
        assert_eq!(today_titles(&e, t0), ["b", "c", "a"]);
        assert_eq!((e.timer.task_id, e.timer.run), (Some(b), RunState::Running));
    }

    #[test]
    fn set_task_matches_today_or_adds_to_the_top() {
        let mut e = engine();
        let t0 = now();
        add_task(&e, "a", Bucket::Today, t0);
        let b = add_task(&e, "Tailor Resume", Bucket::Today, t0);
        e.apply(Action::SetTask(Some("tailor resume".into())), t0);
        assert_eq!(e.timer.task_id, Some(b));
        e.apply(Action::SetTask(Some("Call recruiter 15m #job".into())), t0);
        assert_eq!(today_titles(&e, t0), ["Call recruiter", "a", "Tailor Resume"]);
        assert_eq!(e.snapshot(t0).task_estimate_ms, Some(15 * MINUTE));
        e.apply(Action::SetTask(None), t0);
        assert_eq!((e.timer.task_id, e.timer.task.as_deref()), (None, None));
    }

    #[test]
    fn live_task_moved_away_is_credited_deleted_is_not() {
        let mut e = engine();
        let t0 = now();
        let a = add_task(&e, "a", Bucket::Today, t0);
        let b = add_task(&e, "b", Bucket::Today, t0);
        let c = add_task(&e, "c", Bucket::Today, t0);
        e.apply(Action::Toggle, t0);
        e.db.move_task(a, Bucket::Week, None, t0 + 5 * MINUTE).unwrap();
        e.reconcile_live(t0 + 5 * MINUTE);
        assert_eq!(e.timer.task_id, Some(b));
        e.db.delete_task(b, t0).unwrap();
        e.reconcile_live(t0 + 9 * MINUTE);
        assert_eq!(e.timer.task_id, Some(c));
        e.db.update_task(c, &tasks::parse("c renamed #x").unwrap()).unwrap();
        e.reconcile_live(t0 + 10 * MINUTE);
        assert_eq!((e.timer.task.as_deref(), e.timer.project.as_deref()), (Some("c renamed"), Some("x")));
        let f = e.tick(t0 + 25 * MINUTE).unwrap();
        assert_eq!(f.task_ms, vec![(a, 5 * MINUTE), (c, 16 * MINUTE)]);
    }

    #[test]
    fn task_state_survives_restart() {
        let dir = std::env::temp_dir().join(format!("chronos-task-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.sqlite3");
        let _ = std::fs::remove_file(&path);
        let t0 = now();
        let (a, b);
        {
            let mut e = Engine::load(Db::open(&path).unwrap());
            a = add_task(&e, "a", Bucket::Today, t0);
            b = add_task(&e, "b", Bucket::Today, t0);
            e.apply(Action::Toggle, t0);
            e.apply(Action::DoneTask, t0 + 10 * MINUTE);
        }
        let mut e = Engine::load(Db::open(&path).unwrap());
        assert_eq!(e.timer.task_id, Some(b));
        e.tick(t0 + 3 * 60 * MINUTE).expect("completed on relaunch");
        assert_eq!((spent(&e, a, t0), spent(&e, b, t0)), (10 * MINUTE, 15 * MINUTE));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn day_bounds_cover_a_day() {
        let (a, b) = day_bounds(now_ms());
        assert!(a <= now_ms() && now_ms() < b);
        assert!((23 * 60..=25 * 60).contains(&((b - a) / MINUTE)));
    }
}
