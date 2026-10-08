//! Pomodoro state machine.
//!
//! Pure and deterministic: every transition receives `now` (epoch ms), nothing
//! here reads a clock or touches I/O. Time is tracked as an absolute `end_at`
//! while running, so sleep, freezes and hidden windows can't cause drift — the
//! remaining time is always `end_at - now`.

use serde::{Deserialize, Serialize};

pub const MINUTE: i64 = 60_000;
/// Blocks that ran for less than this are treated as mis-clicks.
pub const CANCEL_THRESHOLD_MS: i64 = MINUTE;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Focus,
    ShortBreak,
    LongBreak,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Focus => "focus",
            Phase::ShortBreak => "short_break",
            Phase::LongBreak => "long_break",
        }
    }

    pub fn parse(s: &str) -> Option<Phase> {
        match s {
            "focus" => Some(Phase::Focus),
            "short_break" => Some(Phase::ShortBreak),
            "long_break" => Some(Phase::LongBreak),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Idle,
    Running,
    Paused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndStatus {
    Completed,
    Abandoned,
    Cancelled,
}

impl EndStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            EndStatus::Completed => "completed",
            EndStatus::Abandoned => "abandoned",
            EndStatus::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Durations {
    pub focus_ms: i64,
    pub short_ms: i64,
    pub long_ms: i64,
    pub long_every: u32,
}

impl Durations {
    pub fn for_phase(&self, phase: Phase) -> i64 {
        match phase {
            Phase::Focus => self.focus_ms,
            Phase::ShortBreak => self.short_ms,
            Phase::LongBreak => self.long_ms,
        }
    }
}

/// A block that just ended, ready to be written to the sessions table.
#[derive(Clone, Debug, PartialEq)]
pub struct Finished {
    pub session_id: Option<i64>,
    pub phase: Phase,
    pub status: EndStatus,
    pub started_at: i64,
    pub ended_at: i64,
    pub planned_ms: i64,
    pub actual_ms: i64,
    pub pause_count: u32,
    pub adjust_ms: i64,
    pub task: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Timer {
    pub phase: Phase,
    pub run: RunState,
    /// Planned length of the current block, including live adjustments.
    pub duration_ms: i64,
    /// Authoritative while idle or paused.
    pub remaining_ms: i64,
    /// Authoritative while running.
    pub end_at: Option<i64>,
    /// First start of the current block; `None` until it is started.
    pub started_at: Option<i64>,
    /// Start of the current uninterrupted running stretch.
    pub run_since: Option<i64>,
    /// Running time accumulated before `run_since`.
    pub active_ms: i64,
    pub pause_count: u32,
    pub adjust_ms: i64,
    /// Focus blocks completed since the last long break.
    pub cycle: u32,
    pub task: Option<String>,
    pub session_id: Option<i64>,
}

impl Default for Timer {
    fn default() -> Self {
        Timer::new(25 * MINUTE)
    }
}

impl Timer {
    pub fn new(focus_ms: i64) -> Self {
        Timer {
            phase: Phase::Focus,
            run: RunState::Idle,
            duration_ms: focus_ms,
            remaining_ms: focus_ms,
            end_at: None,
            started_at: None,
            run_since: None,
            active_ms: 0,
            pause_count: 0,
            adjust_ms: 0,
            cycle: 0,
            task: None,
            session_id: None,
        }
    }

    pub fn is_started(&self) -> bool {
        self.started_at.is_some()
    }

    pub fn remaining(&self, now: i64) -> i64 {
        match (self.run, self.end_at) {
            (RunState::Running, Some(end)) => (end - now).max(0),
            _ => self.remaining_ms,
        }
    }

    pub fn active(&self, now: i64) -> i64 {
        match (self.run, self.run_since) {
            (RunState::Running, Some(since)) => self.active_ms + (now - since).max(0),
            _ => self.active_ms,
        }
    }

    /// Starts a fresh block or resumes a paused one. Returns true when a new
    /// block began (the caller should create its session row).
    pub fn start(&mut self, now: i64) -> bool {
        match self.run {
            RunState::Running => false,
            RunState::Paused => {
                self.end_at = Some(now + self.remaining_ms);
                self.run_since = Some(now);
                self.run = RunState::Running;
                false
            }
            RunState::Idle => {
                self.started_at = Some(now);
                self.end_at = Some(now + self.remaining_ms);
                self.run_since = Some(now);
                self.active_ms = 0;
                self.pause_count = 0;
                self.adjust_ms = 0;
                self.run = RunState::Running;
                true
            }
        }
    }

    pub fn pause(&mut self, now: i64) {
        if self.run != RunState::Running {
            return;
        }
        self.remaining_ms = self.remaining(now);
        self.active_ms = self.active(now);
        self.end_at = None;
        self.run_since = None;
        self.pause_count += 1;
        self.run = RunState::Paused;
    }

    /// Called periodically. Completes the block if its end time has passed;
    /// the block is recorded as ending at `end_at`, not at `now`, so a laptop
    /// that slept through the end still gets an accurate record.
    pub fn tick(&mut self, now: i64, d: &Durations) -> Option<Finished> {
        match (self.run, self.end_at) {
            (RunState::Running, Some(end)) if now >= end => {
                let f = self.finish(end, Some(EndStatus::Completed));
                self.advance(true, d);
                f
            }
            _ => None,
        }
    }

    /// Back to the full duration of the current phase, idle.
    pub fn reset(&mut self, now: i64, d: &Durations) -> Option<Finished> {
        let f = self.finish(now, None);
        self.load_phase(self.phase, d);
        f
    }

    /// End the current block early and move to the next phase.
    pub fn skip(&mut self, now: i64, d: &Durations) -> Option<Finished> {
        let f = self.finish(now, None);
        self.advance(false, d);
        f
    }

    pub fn switch_phase(&mut self, phase: Phase, now: i64, d: &Durations) -> Option<Finished> {
        let f = self.finish(now, None);
        if self.phase == Phase::LongBreak {
            // Leaving a long break early still starts a new cycle.
            self.cycle = 0;
        }
        self.load_phase(phase, d);
        f
    }

    /// Add (positive) or remove (negative) time without restarting.
    /// Removing more time than remains completes a started block.
    pub fn adjust(&mut self, delta_ms: i64, now: i64, d: &Durations) -> Option<Finished> {
        match self.run {
            RunState::Idle => {
                let v = (self.remaining_ms + delta_ms).max(MINUTE);
                self.remaining_ms = v;
                self.duration_ms = v;
                None
            }
            RunState::Running => {
                let end = self.end_at.unwrap_or(now) + delta_ms;
                self.end_at = Some(end);
                self.duration_ms += delta_ms;
                self.adjust_ms += delta_ms;
                if end <= now {
                    self.end_at = Some(now);
                    return self.tick(now, d);
                }
                None
            }
            RunState::Paused => {
                self.remaining_ms += delta_ms;
                self.duration_ms += delta_ms;
                self.adjust_ms += delta_ms;
                if self.remaining_ms <= 0 {
                    self.remaining_ms = 0;
                    let f = self.finish(now, Some(EndStatus::Completed));
                    self.advance(true, d);
                    return f;
                }
                None
            }
        }
    }

    /// Set the remaining time to an exact value (minimum one second).
    pub fn set_remaining(&mut self, ms: i64, now: i64) {
        let ms = ms.max(1_000);
        match self.run {
            RunState::Idle => {
                self.remaining_ms = ms;
                self.duration_ms = ms;
            }
            RunState::Running | RunState::Paused => {
                let delta = ms - self.remaining(now);
                self.duration_ms += delta;
                self.adjust_ms += delta;
                if self.run == RunState::Running {
                    self.end_at = Some(now + ms);
                } else {
                    self.remaining_ms = ms;
                }
            }
        }
    }

    /// New default durations (settings or preset changed). Only an untouched
    /// idle block picks them up; a block in progress keeps its plan.
    pub fn apply_durations(&mut self, d: &Durations) {
        if self.run == RunState::Idle {
            self.load_phase(self.phase, d);
        }
    }

    fn load_phase(&mut self, phase: Phase, d: &Durations) {
        let task = self.task.take();
        let cycle = self.cycle;
        *self = Timer::new(d.for_phase(phase));
        self.phase = phase;
        self.cycle = cycle;
        // The task describes what you're working on, so it survives breaks.
        self.task = task;
    }

    fn advance(&mut self, completed: bool, d: &Durations) {
        let next = match self.phase {
            Phase::Focus => {
                if completed {
                    self.cycle += 1;
                }
                if completed && d.long_every > 0 && self.cycle >= d.long_every {
                    Phase::LongBreak
                } else {
                    Phase::ShortBreak
                }
            }
            Phase::LongBreak => {
                self.cycle = 0;
                Phase::Focus
            }
            Phase::ShortBreak => Phase::Focus,
        };
        self.load_phase(next, d);
    }

    fn finish(&self, ended_at: i64, status: Option<EndStatus>) -> Option<Finished> {
        let started_at = self.started_at?;
        let actual_ms = self.active(ended_at);
        let status = status.unwrap_or(if actual_ms < CANCEL_THRESHOLD_MS {
            EndStatus::Cancelled
        } else {
            EndStatus::Abandoned
        });
        Some(Finished {
            session_id: self.session_id,
            phase: self.phase,
            status,
            started_at,
            ended_at,
            planned_ms: self.duration_ms,
            actual_ms,
            pause_count: self.pause_count,
            adjust_ms: self.adjust_ms,
            task: self.task.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: Durations = Durations {
        focus_ms: 25 * MINUTE,
        short_ms: 5 * MINUTE,
        long_ms: 15 * MINUTE,
        long_every: 4,
    };
    const T0: i64 = 1_700_000_000_000;

    fn timer() -> Timer {
        Timer::new(D.focus_ms)
    }

    #[test]
    fn counts_down_from_wall_clock() {
        let mut t = timer();
        assert!(t.start(T0));
        assert_eq!(t.remaining(T0 + 60_000), 24 * MINUTE);
        // A long gap (sleep, frozen UI) doesn't drift: it's just arithmetic.
        assert_eq!(t.remaining(T0 + 20 * MINUTE), 5 * MINUTE);
        assert_eq!(t.remaining(T0 + 99 * MINUTE), 0);
    }

    #[test]
    fn pause_and_resume_preserve_remaining_and_active_time() {
        let mut t = timer();
        t.start(T0);
        t.pause(T0 + 10 * MINUTE);
        assert_eq!(t.run, RunState::Paused);
        assert_eq!(t.remaining(T0 + 50 * MINUTE), 15 * MINUTE);
        assert!(!t.start(T0 + 50 * MINUTE), "resume is not a new block");
        assert_eq!(t.remaining(T0 + 55 * MINUTE), 10 * MINUTE);
        assert_eq!(t.active(T0 + 55 * MINUTE), 15 * MINUTE);
        assert_eq!(t.pause_count, 1);
    }

    #[test]
    fn completes_at_end_time_even_if_ticked_late() {
        let mut t = timer();
        t.start(T0);
        assert!(t.tick(T0 + 24 * MINUTE, &D).is_none());
        // Laptop slept: first tick after waking is an hour late.
        let f = t.tick(T0 + 85 * MINUTE, &D).expect("finished");
        assert_eq!(f.status, EndStatus::Completed);
        assert_eq!(f.ended_at, T0 + 25 * MINUTE);
        assert_eq!(f.actual_ms, 25 * MINUTE);
        assert_eq!(t.phase, Phase::ShortBreak);
        assert_eq!(t.run, RunState::Idle);
        assert_eq!(t.remaining_ms, 5 * MINUTE);
        assert_eq!(t.cycle, 1);
    }

    #[test]
    fn long_break_after_n_focus_blocks_then_cycle_resets() {
        let mut t = timer();
        let mut now = T0;
        for i in 1..=4 {
            t.start(now);
            now += 25 * MINUTE;
            t.tick(now, &D).unwrap();
            let expect = if i == 4 { Phase::LongBreak } else { Phase::ShortBreak };
            assert_eq!(t.phase, expect, "after focus #{i}");
            t.start(now);
            now += t.duration_ms;
            t.tick(now, &D).unwrap();
            assert_eq!(t.phase, Phase::Focus);
        }
        assert_eq!(t.cycle, 0);
    }

    #[test]
    fn extend_and_reduce_while_running() {
        let mut t = timer();
        t.start(T0);
        let now = T0 + 6 * MINUTE + 18_000; // 18:42 left
        assert_eq!(t.remaining(now), 18 * MINUTE + 42_000);
        t.adjust(5 * MINUTE, now, &D);
        assert_eq!(t.remaining(now), 23 * MINUTE + 42_000);
        t.adjust(-10 * MINUTE, now, &D);
        assert_eq!(t.remaining(now), 13 * MINUTE + 42_000);
        assert_eq!(t.duration_ms, 20 * MINUTE);
        assert_eq!(t.adjust_ms, -5 * MINUTE);
        assert_eq!(t.run, RunState::Running);
    }

    #[test]
    fn removing_more_than_remains_completes() {
        let mut t = timer();
        t.start(T0);
        let now = T0 + 22 * MINUTE;
        let f = t.adjust(-5 * MINUTE, now, &D).expect("completed");
        assert_eq!(f.status, EndStatus::Completed);
        assert_eq!(f.actual_ms, 22 * MINUTE);
        assert_eq!(t.phase, Phase::ShortBreak);
    }

    #[test]
    fn idle_adjust_clamps_to_one_minute() {
        let mut t = timer();
        t.adjust(-60 * MINUTE, T0, &D);
        assert_eq!(t.remaining_ms, MINUTE);
        assert_eq!(t.duration_ms, MINUTE);
    }

    #[test]
    fn set_remaining_while_running_keeps_session() {
        let mut t = timer();
        t.start(T0);
        t.session_id = Some(7);
        t.set_remaining(40 * MINUTE, T0 + 5 * MINUTE);
        assert_eq!(t.remaining(T0 + 5 * MINUTE), 40 * MINUTE);
        assert_eq!(t.duration_ms, 45 * MINUTE);
        assert_eq!(t.session_id, Some(7));
        assert_eq!(t.started_at, Some(T0));
    }

    #[test]
    fn early_reset_is_cancelled_later_reset_is_abandoned() {
        let mut t = timer();
        t.start(T0);
        let f = t.reset(T0 + 30_000, &D).unwrap();
        assert_eq!(f.status, EndStatus::Cancelled);
        assert_eq!(t.run, RunState::Idle);
        assert_eq!(t.remaining_ms, 25 * MINUTE);

        t.start(T0);
        let f = t.reset(T0 + 10 * MINUTE, &D).unwrap();
        assert_eq!(f.status, EndStatus::Abandoned);
        assert_eq!(f.actual_ms, 10 * MINUTE);
        assert_eq!(t.phase, Phase::Focus);
    }

    #[test]
    fn reset_before_start_records_nothing() {
        let mut t = timer();
        assert!(t.reset(T0, &D).is_none());
        assert!(t.skip(T0, &D).is_none());
        assert_eq!(t.phase, Phase::ShortBreak);
    }

    #[test]
    fn skipping_focus_does_not_advance_cycle() {
        let mut t = timer();
        t.start(T0);
        let f = t.skip(T0 + 12 * MINUTE, &D).unwrap();
        assert_eq!(f.status, EndStatus::Abandoned);
        assert_eq!(t.phase, Phase::ShortBreak);
        assert_eq!(t.cycle, 0);
    }

    #[test]
    fn switch_phase_mid_session() {
        let mut t = timer();
        t.task = Some("resume".into());
        t.start(T0);
        let f = t.switch_phase(Phase::LongBreak, T0 + 3 * MINUTE, &D).unwrap();
        assert_eq!(f.phase, Phase::Focus);
        assert_eq!(t.phase, Phase::LongBreak);
        assert_eq!(t.remaining_ms, 15 * MINUTE);
        assert_eq!(t.task.as_deref(), Some("resume"));
    }

    #[test]
    fn paused_time_is_not_focused_time() {
        let mut t = timer();
        t.start(T0);
        t.pause(T0 + 5 * MINUTE);
        t.start(T0 + 65 * MINUTE);
        let f = t.tick(T0 + 200 * MINUTE, &D).unwrap();
        assert_eq!(f.actual_ms, 25 * MINUTE);
        assert_eq!(f.ended_at, T0 + 85 * MINUTE);
        assert_eq!(f.pause_count, 1);
    }

    #[test]
    fn new_durations_only_apply_to_untouched_blocks() {
        let deep = Durations { focus_ms: 50 * MINUTE, ..D };
        let mut t = timer();
        t.apply_durations(&deep);
        assert_eq!(t.remaining_ms, 50 * MINUTE);
        t.start(T0);
        t.apply_durations(&D);
        assert_eq!(t.duration_ms, 50 * MINUTE);
    }

    #[test]
    fn state_round_trips_through_json() {
        let mut t = timer();
        t.start(T0);
        t.task = Some("x".into());
        let back: Timer = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(back, t);
    }
}
