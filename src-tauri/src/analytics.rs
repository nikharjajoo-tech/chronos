//! Turns raw sessions into the weekly / monthly picture. Pure functions:
//! callers supply local-day boundaries and an hour-of-day mapper, so the
//! maths is testable without a timezone.

use crate::db::{DistractionRow, SessionRow};
use chrono::{Datelike, Duration, NaiveDate};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayAgg {
    /// Local midnight, epoch ms.
    pub start: i64,
    pub focus_ms: i64,
    pub completed: u32,
    pub abandoned: u32,
    pub distractions: u32,
    /// Day hasn't happened yet (rest of the current week/month).
    pub future: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub focus_ms: i64,
    pub focus_sessions: u32,
    pub completed: u32,
    pub abandoned: u32,
    pub distractions: u32,
    /// completed / (completed + abandoned), 0..1
    pub completion_rate: f64,
    pub avg_session_ms: i64,
    /// Focus time per elapsed day (future days excluded).
    pub avg_daily_focus_ms: i64,
    pub active_days: u32,
    pub elapsed_days: u32,
    /// active_days / elapsed_days, 0..1
    pub consistency: f64,
    pub best_day: Option<i64>,
    pub best_day_ms: i64,
    /// Average end-of-session rating, 1..4.
    pub avg_rating: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Named {
    pub name: String,
    pub value: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodStats {
    pub from: i64,
    pub to: i64,
    pub days: Vec<DayAgg>,
    pub totals: Totals,
    pub previous: Totals,
    /// Focus ms per project, largest first ("" = no project).
    pub projects: Vec<Named>,
    /// Distraction counts per category, largest first ("" = not specified).
    pub categories: Vec<Named>,
    /// Focus ms started in each hour of the day, 0..24.
    pub focus_by_hour: Vec<i64>,
    pub distractions_by_hour: Vec<u32>,
    pub streak: Streak,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Streak {
    /// Consecutive days with ≥1 completed focus session, ending today
    /// (or yesterday, if today has none yet: the streak isn't broken until
    /// the day is over).
    pub current: u32,
    pub best: u32,
    pub today_done: bool,
}

/// Which day (index into `days`) a timestamp falls in.
fn day_index(days: &[(i64, i64)], t: i64) -> Option<usize> {
    days.iter().position(|&(a, b)| t >= a && t < b)
}

pub fn totals(days: &[(i64, i64)], now: i64, sessions: &[SessionRow], distractions: &[DistractionRow]) -> (Vec<DayAgg>, Totals) {
    let mut agg: Vec<DayAgg> = days.iter().map(|&(a, _)| DayAgg { start: a, future: a > now, ..Default::default() }).collect();
    let mut t = Totals::default();
    let mut ratings = (0u32, 0u32);
    for s in sessions.iter().filter(|s| s.is_focus()) {
        let Some(i) = day_index(days, s.started_at) else { continue };
        agg[i].focus_ms += s.actual_ms;
        if s.is_completed() {
            agg[i].completed += 1;
        } else {
            agg[i].abandoned += 1;
        }
        if let Some(r) = s.rating {
            ratings = (ratings.0 + r as u32, ratings.1 + 1);
        }
    }
    for d in distractions {
        if let Some(i) = day_index(days, d.at) {
            agg[i].distractions += 1;
        }
    }
    for d in &agg {
        t.focus_ms += d.focus_ms;
        t.completed += d.completed;
        t.abandoned += d.abandoned;
        t.distractions += d.distractions;
        if !d.future {
            t.elapsed_days += 1;
        }
        if d.completed > 0 {
            t.active_days += 1;
        }
        if d.focus_ms > t.best_day_ms {
            t.best_day_ms = d.focus_ms;
            t.best_day = Some(d.start);
        }
    }
    t.focus_sessions = t.completed + t.abandoned;
    if t.focus_sessions > 0 {
        t.completion_rate = t.completed as f64 / t.focus_sessions as f64;
        t.avg_session_ms = t.focus_ms / t.focus_sessions as i64;
    }
    if t.elapsed_days > 0 {
        t.avg_daily_focus_ms = t.focus_ms / t.elapsed_days as i64;
        t.consistency = t.active_days as f64 / t.elapsed_days as f64;
    }
    if ratings.1 > 0 {
        t.avg_rating = Some(ratings.0 as f64 / ratings.1 as f64);
    }
    (agg, t)
}

pub fn breakdowns(
    sessions: &[SessionRow],
    distractions: &[DistractionRow],
    hour_of: impl Fn(i64) -> usize,
) -> (Vec<Named>, Vec<Named>, Vec<i64>, Vec<u32>) {
    let mut projects: HashMap<String, i64> = HashMap::new();
    let mut focus_by_hour = vec![0i64; 24];
    for s in sessions.iter().filter(|s| s.is_focus()) {
        *projects.entry(s.project.clone().unwrap_or_default()).or_default() += s.actual_ms;
        focus_by_hour[hour_of(s.started_at).min(23)] += s.actual_ms;
    }
    let mut categories: HashMap<String, i64> = HashMap::new();
    let mut distractions_by_hour = vec![0u32; 24];
    for d in distractions {
        *categories.entry(d.category.clone().unwrap_or_default()).or_default() += 1;
        distractions_by_hour[hour_of(d.at).min(23)] += 1;
    }
    let sorted = |m: HashMap<String, i64>| {
        let mut v: Vec<Named> = m.into_iter().map(|(name, value)| Named { name, value }).collect();
        v.sort_by(|a, b| b.value.cmp(&a.value).then(a.name.cmp(&b.name)));
        v
    };
    (sorted(projects), sorted(categories), focus_by_hour, distractions_by_hour)
}

pub fn streak(active: &BTreeSet<NaiveDate>, today: NaiveDate) -> Streak {
    let today_done = active.contains(&today);
    let mut current = 0;
    let mut day = if today_done { today } else { today - Duration::days(1) };
    while active.contains(&day) {
        current += 1;
        day -= Duration::days(1);
    }
    let mut best = 0;
    let mut run = 0;
    let mut prev: Option<NaiveDate> = None;
    for &d in active {
        run = if prev.map(|p| p + Duration::days(1)) == Some(d) { run + 1 } else { 1 };
        best = best.max(run);
        prev = Some(d);
    }
    Streak { current, best, today_done }
}

/// First and last+1 calendar dates of the week (Mon–Sun) or month that is
/// `offset` periods away from the one containing `today` (offset ≤ 0).
pub fn period_dates(kind: &str, offset: i32, today: NaiveDate) -> (NaiveDate, NaiveDate) {
    if kind == "month" {
        let months = today.year() * 12 + today.month0() as i32 + offset;
        let first = NaiveDate::from_ymd_opt(months.div_euclid(12), months.rem_euclid(12) as u32 + 1, 1).unwrap_or(today);
        let next = NaiveDate::from_ymd_opt((months + 1).div_euclid(12), (months + 1).rem_euclid(12) as u32 + 1, 1).unwrap_or(today);
        (first, next)
    } else {
        let monday = today - Duration::days(today.weekday().num_days_from_monday() as i64) + Duration::weeks(offset as i64);
        (monday, monday + Duration::days(7))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = 3_600_000;
    const DAY: i64 = 24 * H;

    fn row(start: i64, mins: i64, completed: bool, project: Option<&str>, rating: Option<u8>) -> SessionRow {
        SessionRow {
            id: start,
            kind: "focus".into(),
            status: if completed { "completed" } else { "abandoned" }.into(),
            started_at: start,
            ended_at: Some(start + mins * 60_000),
            planned_ms: 25 * 60_000,
            actual_ms: mins * 60_000,
            pause_count: 0,
            away_count: 0,
            task: None,
            project: project.map(String::from),
            rating,
            goal_completed: None,
        }
    }

    fn distraction(at: i64, category: Option<&str>) -> DistractionRow {
        DistractionRow { id: at, at, category: category.map(String::from), session_id: None }
    }

    #[test]
    fn week_totals_and_consistency() {
        let days: Vec<(i64, i64)> = (0..7).map(|i| (i * DAY, (i + 1) * DAY)).collect();
        let now = 3 * DAY + 12 * H; // Thursday noon → 4 elapsed days
        let sessions = vec![
            row(9 * H, 25, true, Some("cv"), Some(4)),
            row(10 * H, 50, true, Some("cv"), Some(2)),
            row(DAY + 9 * H, 12, false, None, None),
            row(3 * DAY + 9 * H, 25, true, Some("blog"), None),
        ];
        let distractions = vec![distraction(9 * H + 1, Some("phone")), distraction(DAY + 15 * H, None)];
        let (agg, t) = totals(&days, now, &sessions, &distractions);
        assert_eq!(agg.len(), 7);
        assert!(agg[4].future && !agg[3].future);
        assert_eq!(agg[0].focus_ms, 75 * 60_000);
        assert_eq!((t.completed, t.abandoned, t.focus_sessions), (3, 1, 4));
        assert_eq!(t.focus_ms, 112 * 60_000);
        assert_eq!(t.elapsed_days, 4);
        assert_eq!(t.active_days, 2, "day 1 only had an abandoned session");
        assert!((t.consistency - 0.5).abs() < 1e-9);
        assert!((t.completion_rate - 0.75).abs() < 1e-9);
        assert_eq!(t.avg_daily_focus_ms, 28 * 60_000);
        assert_eq!(t.best_day, Some(0));
        assert_eq!(t.distractions, 2);
        assert_eq!(t.avg_rating, Some(3.0));
    }

    #[test]
    fn breakdowns_by_project_category_and_hour() {
        let sessions = vec![row(9 * H, 25, true, Some("cv"), None), row(14 * H, 50, true, None, None), row(9 * H + 1, 25, true, Some("cv"), None)];
        let distractions = vec![distraction(14 * H, Some("youtube")), distraction(15 * H, Some("youtube")), distraction(15 * H + 5, None)];
        let (projects, categories, fh, dh) = breakdowns(&sessions, &distractions, |t| ((t / H) % 24) as usize);
        assert_eq!(projects[0], Named { name: "".into(), value: 50 * 60_000 });
        assert_eq!(projects[1], Named { name: "cv".into(), value: 50 * 60_000 });
        assert_eq!(categories[0], Named { name: "youtube".into(), value: 2 });
        assert_eq!(fh[9], 50 * 60_000);
        assert_eq!(dh[15], 2);
    }

    #[test]
    fn streaks() {
        let d = |m, day| NaiveDate::from_ymd_opt(2026, m, day).unwrap();
        let active: BTreeSet<_> = [d(9, 28), d(9, 29), d(9, 30), d(10, 1), d(10, 5), d(10, 6), d(10, 7)].into_iter().collect();
        // Today (Oct 8) has nothing yet: the streak through yesterday still counts.
        assert_eq!(streak(&active, d(10, 8)), Streak { current: 3, best: 4, today_done: false });
        assert_eq!(streak(&active, d(10, 7)).current, 3);
        assert_eq!(streak(&active, d(10, 10)).current, 0);
    }

    #[test]
    fn period_boundaries() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap(); // Thursday
        let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day).unwrap();
        assert_eq!(period_dates("week", 0, today), (d(2026, 10, 5), d(2026, 10, 12)));
        assert_eq!(period_dates("week", -1, today), (d(2026, 9, 28), d(2026, 10, 5)));
        assert_eq!(period_dates("month", 0, today), (d(2026, 10, 1), d(2026, 11, 1)));
        assert_eq!(period_dates("month", -10, today), (d(2025, 12, 1), d(2026, 1, 1)));
    }
}
