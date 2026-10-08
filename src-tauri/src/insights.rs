//! The coaching layer: a weekly score, data-backed insights and at most one
//! suggestion. Pure functions over sessions so every rule is testable; the
//! caller supplies local-time helpers (hour, weekday) to keep tz out of here.
//!
//! Rule of thumb throughout: say nothing unless there's enough data, and
//! always show the evidence behind what is said.

use crate::analytics::Totals;
use crate::db::{DistractionRow, SessionRow};
use crate::settings::Settings;
use serde::Serialize;

const MIN: i64 = 60_000;

// ── 1. Productivity score ──────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScorePart {
    pub key: &'static str,
    pub label: &'static str,
    /// Points earned and available (available is rescaled when a part is missing).
    pub points: f64,
    pub max: f64,
    /// Plain-language evidence, e.g. "4 of 5 days".
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Score {
    pub value: u32,
    pub parts: Vec<ScorePart>,
}

/// Fraction of completed focus sessions that were followed by a real break:
/// a break session, or simply no new focus within 5 minutes. Returns
/// (taken, total).
pub fn breaks_taken(sessions: &[SessionRow]) -> (u32, u32) {
    let mut sorted: Vec<&SessionRow> = sessions.iter().collect();
    sorted.sort_by_key(|s| s.started_at);
    let (mut taken, mut total) = (0, 0);
    for (i, s) in sorted.iter().enumerate() {
        if !(s.is_focus() && s.is_completed()) {
            continue;
        }
        total += 1;
        let end = s.ended_at.unwrap_or(s.started_at + s.actual_ms);
        let skipped = sorted.get(i + 1).is_some_and(|n| n.is_focus() && n.started_at - end < 5 * MIN);
        if !skipped {
            taken += 1;
        }
    }
    (taken, total)
}

pub fn score(t: &Totals, sessions: &[SessionRow], daily_goal_ms: i64) -> Option<Score> {
    if t.focus_sessions == 0 || t.elapsed_days == 0 {
        return None;
    }
    let goal = if daily_goal_ms > 0 { daily_goal_ms } else { 120 * MIN };
    let focus_hours = t.focus_ms as f64 / (60.0 * MIN as f64);
    let per_hour = if focus_hours > 0.0 { t.distractions as f64 / focus_hours } else { 0.0 };
    let (taken, breaks) = breaks_taken(sessions);

    // (key, label, weight, 0..1 value, detail)
    let mut raw: Vec<(&'static str, &'static str, f64, Option<f64>, String)> = vec![
        (
            "focus",
            "Focus time",
            25.0,
            Some((t.avg_daily_focus_ms as f64 / goal as f64).min(1.0)),
            format!("{}/day of {} goal", hm(t.avg_daily_focus_ms), hm(goal)),
        ),
        (
            "consistency",
            "Consistency",
            20.0,
            Some(t.consistency),
            format!("{} of {} days", t.active_days, t.elapsed_days),
        ),
        (
            "completion",
            "Finishing sessions",
            20.0,
            Some(t.completion_rate),
            format!("{} of {} finished", t.completed, t.focus_sessions),
        ),
        (
            "distractions",
            "Staying on task",
            15.0,
            // 0 per focus hour = full marks; 4+ per hour = none.
            Some((1.0 - per_hour / 4.0).clamp(0.0, 1.0)),
            format!("{per_hour:.1} distractions per focus hour"),
        ),
        (
            "breaks",
            "Taking breaks",
            10.0,
            (breaks > 0).then(|| taken as f64 / breaks as f64),
            format!("{taken} of {breaks} breaks taken"),
        ),
        (
            "quality",
            "Session quality",
            10.0,
            t.avg_rating.map(|r| ((r - 1.0) / 3.0).clamp(0.0, 1.0)),
            t.avg_rating.map(|r| format!("avg rating {r:.1} / 4")).unwrap_or_default(),
        ),
    ];
    raw.retain(|r| r.3.is_some());
    let total_weight: f64 = raw.iter().map(|r| r.2).sum();
    let parts: Vec<ScorePart> = raw
        .into_iter()
        .map(|(key, label, w, v, detail)| {
            let max = w * 100.0 / total_weight;
            ScorePart { key, label, points: max * v.unwrap_or(0.0), max, detail }
        })
        .collect();
    let value = parts.iter().map(|p| p.points).sum::<f64>().round().clamp(0.0, 100.0) as u32;
    Some(Score { value, parts })
}

/// The part that changed most between two scores, as "(label, ±points)".
pub fn biggest_mover(now: &Score, before: &Score) -> Option<(&'static str, f64)> {
    now.parts
        .iter()
        .filter_map(|p| before.parts.iter().find(|b| b.key == p.key).map(|b| (p.label, p.points - b.points)))
        .filter(|(_, d)| d.abs() >= 1.0)
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
}

// ── 2. Insights ────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Insight {
    pub id: &'static str,
    pub text: String,
    /// What the claim rests on, e.g. "34 sessions over 4 weeks".
    pub evidence: String,
}

/// Local-time helpers supplied by the caller.
pub struct Clock<'a> {
    pub hour: &'a dyn Fn(i64) -> usize,
    /// 0 = Monday … 6 = Sunday
    pub weekday: &'a dyn Fn(i64) -> usize,
    /// Days since some epoch, for counting distinct days.
    pub day: &'a dyn Fn(i64) -> i64,
}

const WEEKDAYS: [&str; 7] = ["Mondays", "Tuesdays", "Wednesdays", "Thursdays", "Fridays", "Saturdays", "Sundays"];

pub fn hour_name(h: usize) -> String {
    match h % 24 {
        0 => "12 AM".into(),
        h @ 1..=11 => format!("{h} AM"),
        12 => "12 PM".into(),
        h => format!("{} PM", h - 12),
    }
}

fn hm(ms: i64) -> String {
    let m = (ms + MIN / 2) / MIN;
    match (m / 60, m % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

fn rate(xs: &[&SessionRow]) -> f64 {
    if xs.is_empty() {
        0.0
    } else {
        xs.iter().filter(|s| s.is_completed()).count() as f64 / xs.len() as f64
    }
}

fn median(mut v: Vec<i64>) -> i64 {
    v.sort_unstable();
    v[v.len() / 2]
}

/// `recent`: focus + break sessions over the last 28 days (oldest first),
/// `distractions` over the same window, `now` for splitting halves.
pub fn insights(recent: &[SessionRow], distractions: &[DistractionRow], now: i64, c: &Clock) -> Vec<Insight> {
    let focus: Vec<&SessionRow> = recent.iter().filter(|s| s.is_focus()).collect();
    let mut out = Vec::new();
    let days_span = {
        let mut d: Vec<i64> = focus.iter().map(|s| (c.day)(s.started_at)).collect();
        d.sort_unstable();
        d.dedup();
        d.len()
    };

    // Best start window: a 3-hour block (starting at an hour you actually
    // start sessions) whose sessions finish noticeably more often.
    let mut starts: Vec<usize> = focus.iter().map(|s| (c.hour)(s.started_at)).collect();
    starts.sort_unstable();
    starts.dedup();
    let mut best: Option<(usize, f64, f64, usize)> = None;
    for start in starts {
        let (inside, outside): (Vec<&SessionRow>, Vec<&SessionRow>) =
            focus.iter().partition(|s| ((c.hour)(s.started_at) + 24 - start) % 24 < 3);
        if inside.len() >= 5 && outside.len() >= 5 {
            let (ri, ro) = (rate(&inside), rate(&outside));
            if ri - ro >= 0.15 && best.is_none_or(|b| ri - ro > b.1 - b.2) {
                best = Some((start, ri, ro, inside.len()));
            }
        }
    }
    if let Some((start, ri, ro, n)) = best {
        let more = ((ri / ro.max(0.01)) - 1.0) * 100.0;
        out.push(Insight {
            id: "best-window",
            text: format!(
                "You finish {}% more of your sessions when you start between {} and {}.",
                more.round() as i64,
                hour_name(start),
                hour_name(start + 3)
            ),
            evidence: format!("{}% vs {}% finished · {n} sessions in that window", (ri * 100.0).round(), (ro * 100.0).round()),
        });
    }

    // Where sessions get abandoned.
    let abandoned: Vec<i64> = focus.iter().filter(|s| !s.is_completed() && s.actual_ms >= 3 * MIN).map(|s| s.actual_ms).collect();
    if abandoned.len() >= 4 {
        let n = abandoned.len();
        let m = median(abandoned) / MIN;
        out.push(Insight {
            id: "abandon-point",
            text: format!("When you stop a session early, it's usually around minute {m}."),
            evidence: format!("median of {n} sessions ended early"),
        });
    }

    // Distraction peak: the 2-hour window holding the biggest share.
    if distractions.len() >= 6 {
        let mut by_hour = [0usize; 24];
        for d in distractions {
            by_hour[(c.hour)(d.at).min(23)] += 1;
        }
        // Ties go to the window that starts at the busier hour.
        let (h, n) = (0..23)
            .map(|h| (h, by_hour[h] + by_hour[h + 1]))
            .max_by(|a, b| a.1.cmp(&b.1).then(by_hour[a.0].cmp(&by_hour[b.0])))
            .unwrap_or((0, 0));
        let share = n as f64 / distractions.len() as f64;
        if share >= 0.3 {
            out.push(Insight {
                id: "distraction-peak",
                text: format!("Your distractions peak between {} and {}.", hour_name(h), hour_name(h + 2)),
                evidence: format!("{}% of {} logged distractions", (share * 100.0).round(), distractions.len()),
            });
        }
    }

    // Main reason for distractions.
    let tagged: Vec<&str> = distractions.iter().filter_map(|d| d.category.as_deref()).collect();
    if tagged.len() >= 5 {
        let mut counts: Vec<(&str, usize)> = Vec::new();
        for t in &tagged {
            match counts.iter_mut().find(|c| c.0 == *t) {
                Some(c) => c.1 += 1,
                None => counts.push((t, 1)),
            }
        }
        counts.sort_by(|a, b| b.1.cmp(&a.1));
        let (cat, n) = counts[0];
        let share = n as f64 / tagged.len() as f64;
        if share >= 0.35 {
            let name = match cat {
                "social" => "Social media",
                "youtube" => "YouTube",
                "phone" => "Your phone",
                "messaging" => "Messaging",
                "browsing" => "Random browsing",
                "work" => "Other work",
                _ => "“Other”",
            };
            out.push(Insight {
                id: "top-distraction",
                text: format!("{name} is behind {}% of your distractions.", (share * 100.0).round()),
                evidence: format!("{n} of {} distractions with a reason", tagged.len()),
            });
        }
    }

    // Session length trend: last 14 days vs the 14 before.
    let half = now - 14 * 24 * 60 * MIN;
    let (newer, older): (Vec<&SessionRow>, Vec<&SessionRow>) = focus.iter().partition(|s| s.started_at >= half);
    if newer.len() >= 5 && older.len() >= 5 {
        let avg = |v: &[&SessionRow]| v.iter().map(|s| s.actual_ms).sum::<i64>() / v.len() as i64;
        let (a_old, a_new) = (avg(&older), avg(&newer));
        let change = (a_new - a_old) as f64 / a_old.max(1) as f64;
        if change.abs() >= 0.15 {
            out.push(Insight {
                id: "length-trend",
                text: format!(
                    "Your average session {} from {} to {} minutes.",
                    if change > 0.0 { "grew" } else { "dropped" },
                    a_old / MIN,
                    a_new / MIN
                ),
                evidence: format!("last 2 weeks ({} sessions) vs the 2 before ({})", newer.len(), older.len()),
            });
        }
    }

    // Strongest weekday by average focus on days you worked.
    if days_span >= 8 {
        let mut per_day: Vec<(i64, usize, i64)> = Vec::new(); // (day, weekday, focus_ms)
        for s in &focus {
            let d = (c.day)(s.started_at);
            match per_day.iter_mut().find(|x| x.0 == d) {
                Some(x) => x.2 += s.actual_ms,
                None => per_day.push((d, (c.weekday)(s.started_at), s.actual_ms)),
            }
        }
        let overall = per_day.iter().map(|x| x.2).sum::<i64>() as f64 / per_day.len() as f64;
        let best = (0..7)
            .filter_map(|wd| {
                let v: Vec<i64> = per_day.iter().filter(|x| x.1 == wd).map(|x| x.2).collect();
                (v.len() >= 2).then(|| (wd, v.iter().sum::<i64>() as f64 / v.len() as f64, v.len()))
            })
            .max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((wd, avg, n)) = best {
            if avg >= overall * 1.3 {
                out.push(Insight {
                    id: "best-weekday",
                    text: format!("{} are your strongest days, averaging {} of focus.", WEEKDAYS[wd], hm(avg as i64)),
                    evidence: format!("{n} {} vs {} on an average working day", WEEKDAYS[wd].to_lowercase(), hm(overall as i64)),
                });
            }
        }
    }

    // How rating relates to session length.
    let rated: Vec<&&SessionRow> = focus.iter().filter(|s| s.rating.is_some()).collect();
    let (short, long): (Vec<&&SessionRow>, Vec<&&SessionRow>) = rated.iter().partition(|s| s.actual_ms <= 30 * MIN);
    if short.len() >= 4 && long.len() >= 4 {
        let avg = |v: &[&&SessionRow]| v.iter().map(|s| s.rating.unwrap_or(0) as f64).sum::<f64>() / v.len() as f64;
        let (rs, rl) = (avg(&short), avg(&long));
        if (rs - rl).abs() >= 0.5 {
            let (better, a, b) = if rs > rl { ("sessions of 30 minutes or less", rs, rl) } else { ("sessions longer than 30 minutes", rl, rs) };
            out.push(Insight {
                id: "rating-length",
                text: format!("You rate {better} as more productive."),
                evidence: format!("avg {a:.1} vs {b:.1} out of 4 · {} rated sessions", rated.len()),
            });
        }
    }

    // Stepping away mid-session.
    let away: u32 = focus.iter().filter(|s| s.started_at >= now - 7 * 24 * 60 * MIN).map(|s| s.away_count).sum();
    if away >= 3 {
        out.push(Insight {
            id: "away",
            text: format!("Sessions paused because you stepped away {away} times this week."),
            evidence: "from automatic away detection".into(),
        });
    }

    out
}

// ── 3. One suggestion ──────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestionAction {
    pub label: String,
    /// "focusMin" | "dnd" | "autoStartBreaks"
    pub setting: &'static str,
    pub value: serde_json::Value,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    /// Stable for the exact advice, so a "no thanks" sticks.
    pub id: String,
    pub text: String,
    pub evidence: String,
    pub action: Option<SuggestionAction>,
}

/// `week`: sessions in the last 7 days; `prev_week`: the 7 before.
/// `distractions_week` / `_prev`: counts for the same windows.
pub fn suggestion(
    s: &Settings,
    week: &[SessionRow],
    prev_week: &[SessionRow],
    distractions_week: usize,
    distractions_prev: usize,
    insights: &[Insight],
    dismissed: &[String],
) -> Option<Suggestion> {
    let fresh = |id: &str| !dismissed.iter().any(|d| d == id);
    let cur = s.focus_min as i64;
    // Sessions planned at (roughly) the current focus length.
    let at_len: Vec<&SessionRow> = week
        .iter()
        .filter(|x| x.is_focus() && (x.planned_ms - cur * MIN).abs() <= 2 * MIN)
        .collect();
    let mut out: Vec<Suggestion> = Vec::new();

    // Struggling with the current length → go shorter.
    if at_len.len() >= 6 && rate(&at_len) < 0.6 && cur > 15 {
        let ended: Vec<i64> = at_len.iter().filter(|x| !x.is_completed()).map(|x| x.actual_ms / MIN).collect();
        let target = ((median(ended) / 5) * 5).clamp(15, cur - 5);
        out.push(Suggestion {
            id: format!("focus:{cur}->{target}"),
            text: format!("You finish fewer {cur}-minute sessions than you start. Try {target}-minute sessions for a while?"),
            evidence: format!("{}% of {} sessions finished this week", (rate(&at_len) * 100.0).round(), at_len.len()),
            action: Some(SuggestionAction { label: format!("Switch to {target} min"), setting: "focusMin", value: target.into() }),
        });
    }

    // Distractions rising → Do Not Disturb.
    let focus_ms = |v: &[SessionRow]| v.iter().filter(|x| x.is_focus()).map(|x| x.actual_ms).sum::<i64>();
    let per_hour = |n: usize, ms: i64| if ms > 0 { n as f64 / (ms as f64 / (60.0 * MIN as f64)) } else { 0.0 };
    let (now_rate, prev_rate) = (per_hour(distractions_week, focus_ms(week)), per_hour(distractions_prev, focus_ms(prev_week)));
    if distractions_week >= 5 && prev_rate > 0.0 && now_rate >= prev_rate * 1.25 {
        let up = ((now_rate / prev_rate - 1.0) * 100.0).round();
        out.push(Suggestion {
            id: format!("dnd:{}", if s.dnd { "tip" } else { "on" }),
            text: if s.dnd {
                format!("Distractions are up {up}% this week. A shorter focus length can help when focus is hard to hold.")
            } else {
                format!("Distractions are up {up}% this week. Turn on Do Not Disturb during focus sessions?")
            },
            evidence: format!("{now_rate:.1} vs {prev_rate:.1} per focus hour"),
            action: (!s.dnd).then(|| SuggestionAction { label: "Turn on Do Not Disturb".into(), setting: "dnd", value: true.into() }),
        });
    }

    // Skipping breaks → start them automatically.
    let (taken, total) = breaks_taken(week);
    if total >= 6 && (taken as f64) < total as f64 * 0.6 && !s.auto_start_breaks {
        out.push(Suggestion {
            id: "breaks:auto".into(),
            text: "You've been skipping most of your breaks. Want breaks to start on their own after each session?".into(),
            evidence: format!("{taken} of {total} breaks taken this week"),
            action: Some(SuggestionAction { label: "Start breaks automatically".into(), setting: "autoStartBreaks", value: true.into() }),
        });
    }

    // Comfortably finishing every session → go a little longer.
    let rated: Vec<u8> = at_len.iter().filter_map(|x| x.rating).collect();
    let happy = rated.is_empty() || rated.iter().map(|&r| r as f64).sum::<f64>() / rated.len() as f64 >= 3.0;
    if at_len.len() >= 8 && rate(&at_len) >= 0.9 && happy && cur < 60 {
        let target = (cur + 5).min(60);
        out.push(Suggestion {
            id: format!("focus:{cur}->{target}"),
            text: format!("You've finished nearly every {cur}-minute session this week. Ready to try {target} minutes?"),
            evidence: format!("{} of {} finished", at_len.iter().filter(|x| x.is_completed()).count(), at_len.len()),
            action: Some(SuggestionAction { label: format!("Switch to {target} min"), setting: "focusMin", value: target.into() }),
        });
    }

    // Protect the best hours (advice only).
    if let Some(i) = insights.iter().find(|i| i.id == "best-window") {
        out.push(Suggestion {
            id: format!("protect:{}", i.text),
            text: format!("{} Consider keeping that time for your hardest work.", i.text),
            evidence: i.evidence.clone(),
            action: None,
        });
    }

    out.into_iter().find(|x| fresh(&x.id))
}

// ── 4. Companion lines ─────────────────────────────────────────────────────

/// What the clock may say after a focus session ends. Rare on purpose: the
/// caller rate-limits to one line per 90 minutes (the goal line excepted).
/// Lines stay under ~35 characters so they fit on the smallest widget line.
pub fn companion_line(focus_run_ms: i64, skipped_breaks: u32, goal_just_reached: Option<i64>) -> Option<(bool, String)> {
    if let Some(goal) = goal_just_reached {
        return Some((true, format!("Daily goal hit: {}. Nice work.", hm(goal))));
    }
    if focus_run_ms >= 120 * MIN {
        return Some((false, format!("{} of focus. Take a real break.", hm(focus_run_ms))));
    }
    if skipped_breaks >= 2 {
        return Some((false, "Pushing hard. A break might help.".into()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::Totals;

    const H: i64 = 60 * MIN;
    const DAY: i64 = 24 * H;

    fn focus(start: i64, mins: i64, completed: bool) -> SessionRow {
        SessionRow {
            id: start,
            kind: "focus".into(),
            status: if completed { "completed" } else { "abandoned" }.into(),
            started_at: start,
            ended_at: Some(start + mins * MIN),
            planned_ms: 25 * MIN,
            actual_ms: mins * MIN,
            pause_count: 0,
            away_count: 0,
            task: None,
            project: None,
            rating: None,
            goal_completed: None,
        }
    }

    fn brk(start: i64, mins: i64) -> SessionRow {
        SessionRow { kind: "short_break".into(), status: "completed".into(), ..focus(start, mins, true) }
    }

    fn clock() -> Clock<'static> {
        Clock {
            hour: &|t| ((t / H) % 24) as usize,
            weekday: &|t| ((t / DAY) % 7) as usize,
            day: &|t| t / DAY,
        }
    }

    fn totals(focus_ms: i64, sessions: u32, completed: u32, distractions: u32, active: u32, elapsed: u32, rating: Option<f64>) -> Totals {
        Totals {
            focus_ms,
            focus_sessions: sessions,
            completed,
            abandoned: sessions - completed,
            distractions,
            completion_rate: completed as f64 / sessions as f64,
            avg_daily_focus_ms: focus_ms / elapsed as i64,
            active_days: active,
            elapsed_days: elapsed,
            consistency: active as f64 / elapsed as f64,
            avg_rating: rating,
            ..Default::default()
        }
    }

    #[test]
    fn breaks_taken_counts_back_to_back_focus_as_skipped() {
        let s = vec![
            focus(0, 25, true),
            brk(26 * MIN, 5),
            focus(32 * MIN, 25, true),
            focus(58 * MIN, 25, true), // started 1 min after the previous: skipped
            focus(3 * H, 25, true),    // last one: nothing after it counts as a break
        ];
        assert_eq!(breaks_taken(&s), (3, 4));
    }

    #[test]
    fn perfect_week_scores_100_and_parts_sum() {
        let s = vec![focus(0, 25, true), brk(26 * MIN, 5)];
        let t = totals(3 * H * 5, 5, 5, 0, 5, 5, Some(4.0));
        let sc = score(&t, &s, 3 * H).unwrap();
        assert_eq!(sc.value, 100);
        assert_eq!(sc.parts.len(), 6);
        assert!((sc.parts.iter().map(|p| p.max).sum::<f64>() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn missing_parts_are_rescaled_not_zeroed() {
        // No ratings, no completed focus → no "breaks" or "quality" part.
        let t = totals(2 * H, 2, 0, 0, 0, 2, None);
        let sc = score(&t, &[focus(0, 60, false)], 2 * H).unwrap();
        assert_eq!(sc.parts.len(), 4);
        assert!((sc.parts.iter().map(|p| p.max).sum::<f64>() - 100.0).abs() < 1e-9);
        // 50% of goal, 0 consistency, 0 completion, 0 distractions
        // → (25*.5 + 0 + 0 + 15) / 80 * 100 = 34.4
        assert_eq!(sc.value, 34);
        assert!(score(&Totals::default(), &[], H).is_none());
    }

    #[test]
    fn biggest_mover_names_the_part() {
        let a = Score { value: 80, parts: vec![ScorePart { key: "focus", label: "Focus time", points: 20.0, max: 25.0, detail: "".into() }] };
        let b = Score { value: 70, parts: vec![ScorePart { key: "focus", label: "Focus time", points: 12.0, max: 25.0, detail: "".into() }] };
        assert_eq!(biggest_mover(&a, &b), Some(("Focus time", 8.0)));
    }

    #[test]
    fn best_window_needs_enough_data_and_a_real_gap() {
        // 6 morning sessions all finished, 6 afternoon sessions half finished.
        let mut s = Vec::new();
        for d in 0..6 {
            s.push(focus(d * DAY + 9 * H, 25, true));
            s.push(focus(d * DAY + 15 * H, 25, d % 2 == 0));
        }
        let out = insights(&s, &[], 30 * DAY, &clock());
        let i = out.iter().find(|i| i.id == "best-window").expect("insight");
        assert_eq!(i.text, "You finish 100% more of your sessions when you start between 9 AM and 12 PM.");
        // Too few sessions → silence.
        assert!(insights(&s[..6], &[], 30 * DAY, &clock()).iter().all(|i| i.id != "best-window"));
    }

    #[test]
    fn abandon_point_and_distraction_insights() {
        let mut s: Vec<SessionRow> = (0..5).map(|i| focus(i * DAY, 36 + i, false)).collect();
        s.push(focus(6 * DAY, 2, false)); // too short to count
        let d: Vec<DistractionRow> = (0..8)
            .map(|i| DistractionRow { id: i, at: i * DAY + 14 * H, category: Some(if i < 5 { "youtube" } else { "phone" }.into()), session_id: None })
            .collect();
        let out = insights(&s, &d, 30 * DAY, &clock());
        let text = |id| out.iter().find(|i| i.id == id).map(|i| i.text.clone()).unwrap_or_default();
        assert_eq!(text("abandon-point"), "When you stop a session early, it's usually around minute 38.");
        assert_eq!(text("distraction-peak"), "Your distractions peak between 2 PM and 4 PM.");
        assert_eq!(text("top-distraction"), "YouTube is behind 63% of your distractions.");
    }

    #[test]
    fn suggestions_one_at_a_time_and_never_repeat() {
        let settings = Settings::default();
        let week: Vec<SessionRow> = (0..8).map(|i| focus(i * H * 3, 25, true)).collect();
        let first = suggestion(&settings, &week, &[], 0, 0, &[], &[]).unwrap();
        assert_eq!(first.id, "focus:25->30");
        assert_eq!(first.action.as_ref().unwrap().value, serde_json::json!(30));
        assert!(suggestion(&settings, &week, &[], 0, 0, &[], &[first.id.clone()]).is_none());

        let struggling: Vec<SessionRow> = (0..8).map(|i| focus(i * H * 3, if i < 5 { 18 } else { 25 }, i >= 5)).collect();
        let s = suggestion(&settings, &struggling, &[], 0, 0, &[], &[]).unwrap();
        assert_eq!(s.id, "focus:25->15");
    }

    #[test]
    fn companion_lines_are_prioritised() {
        assert!(companion_line(30 * MIN, 0, None).is_none());
        assert_eq!(companion_line(135 * MIN, 0, None).unwrap().1, "2h 15m of focus. Take a real break.");
        assert!(companion_line(0, 2, None).unwrap().1.contains("Pushing"));
        assert_eq!(companion_line(200 * MIN, 3, Some(3 * H)).unwrap(), (true, "Daily goal hit: 3h. Nice work.".into()));
        for line in [companion_line(599 * MIN, 0, None), companion_line(0, 9, None), companion_line(0, 0, Some(599 * MIN))] {
            assert!(line.unwrap().1.chars().count() <= 35);
        }
    }
}
