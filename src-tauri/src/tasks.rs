//! Planner tasks: parsing what the user types, ordering inside a bucket and the
//! leftover rule. Pure logic; the SQL lives in `db.rs`.

use crate::timer::MINUTE;
use serde::{Deserialize, Serialize};

/// Longest title kept, in characters.
pub const MAX_TITLE: usize = 120;
/// Estimates outside 1 min ..= 8 h stay in the title.
pub const MAX_ESTIMATE_MS: i64 = 8 * 60 * MINUTE;
/// Below this gap between neighbours a bucket is renumbered 1.0, 2.0, …
pub const MIN_GAP: f64 = 1e-6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bucket {
    Today,
    Week,
    Backlog,
}

impl Bucket {
    pub fn as_str(self) -> &'static str {
        match self {
            Bucket::Today => "today",
            Bucket::Week => "week",
            Bucket::Backlog => "backlog",
        }
    }

    pub fn parse(s: &str) -> Option<Bucket> {
        match s {
            "today" => Some(Bucket::Today),
            "week" => Some(Bucket::Week),
            "backlog" => Some(Bucket::Backlog),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRow {
    pub id: i64,
    pub title: String,
    pub project: Option<String>,
    pub bucket: Bucket,
    pub position: f64,
    pub estimate_ms: Option<i64>,
    pub done: bool,
    pub created_at: i64,
    pub moved_at: i64,
    pub done_at: Option<i64>,
    /// Focus time credited so far, including the running block.
    pub spent_ms: i64,
    /// Open Today task carried over from an earlier day.
    pub leftover: bool,
}

/// The result of parsing what the user typed.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedTask {
    pub title: String,
    pub project: Option<String>,
    pub estimate_ms: Option<i64>,
}

/// Splits off the first `#tag` as the project (lower-cased, letters, digits,
/// `-` and `_`, at most 32 chars). Later tags are dropped.
pub fn split_tags(text: &str) -> (Vec<&str>, Option<String>) {
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
    (words, project)
}

/// `"Prep slides 28min #job-search"` → title, project and estimate.
/// The estimate is only taken from the end of the text. None when no title is left.
pub fn parse(text: &str) -> Option<ParsedTask> {
    let (words, project) = split_tags(text);
    // The longest trailing run of words that reads as a duration; one out of
    // range stays in the title whole.
    let (title_words, estimate_ms) = (0..words.len())
        .find_map(|k| parse_duration(&words[k..].join(" ")).map(|ms| (k, ms)))
        .filter(|(_, ms)| (MINUTE..=MAX_ESTIMATE_MS).contains(ms))
        .map_or((&words[..], None), |(k, ms)| (&words[..k], Some(ms)));
    let title: String = title_words.join(" ").chars().take(MAX_TITLE).collect();
    if title.is_empty() {
        return None;
    }
    Some(ParsedTask { title, project, estimate_ms })
}

#[derive(Clone, Copy, PartialEq)]
enum Unit {
    Hours,
    Minutes,
}

/// The whole of `s` as a duration: `28m`, `1.5h`, `1h30`, `2hr 15min`,
/// `1 hour 15 minutes`, rounded to whole minutes. None if anything is left over.
fn parse_duration(s: &str) -> Option<i64> {
    let s = s.to_lowercase();
    let mut rest = s.as_str();
    let mut minutes = 0.0;
    let mut last: Option<Unit> = None;
    while !rest.is_empty() {
        if last == Some(Unit::Minutes) {
            return None;
        }
        let (n, after) = number(rest)?;
        let after = after.trim_start();
        let (unit, after) = match unit(after) {
            Some(u) => u,
            // `1h30`: a bare number straight after the hours means minutes.
            None if after.is_empty() && last == Some(Unit::Hours) => (Unit::Minutes, after),
            None => return None,
        };
        if last == Some(unit) {
            return None;
        }
        minutes += if unit == Unit::Hours { n * 60.0 } else { n };
        last = Some(unit);
        rest = after.trim_start();
    }
    last.map(|_| minutes.round() as i64 * MINUTE)
}

/// A leading decimal number (`.` or `,` as the separator) and what follows it.
fn number(s: &str) -> Option<(f64, &str)> {
    let end = s.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',')).unwrap_or(s.len());
    let n = s[..end].replace(',', ".").parse::<f64>().ok()?;
    Some((n, &s[end..]))
}

/// A leading unit word; the whole run of letters must be a known unit.
fn unit(s: &str) -> Option<(Unit, &str)> {
    let end = s.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(s.len());
    let u = match &s[..end] {
        "h" | "hr" | "hrs" | "hour" | "hours" => Unit::Hours,
        "m" | "min" | "mins" | "minute" | "minutes" => Unit::Minutes,
        _ => return None,
    };
    Some((u, &s[end..]))
}

/// A position strictly between two neighbours (None = no neighbour on that
/// side). None when the gap is too small and the bucket must be renumbered.
pub fn position_between(before: Option<f64>, after: Option<f64>) -> Option<f64> {
    match (before, after) {
        (None, None) => Some(1.0),
        (Some(b), None) => Some(b + 1.0),
        (None, Some(a)) => Some(a - 1.0),
        (Some(b), Some(a)) => (a - b >= MIN_GAP).then_some((a + b) / 2.0),
    }
}

/// An open Today task that entered Today before `today_start`.
pub fn is_leftover(bucket: Bucket, done: bool, moved_at: i64, today_start: i64) -> bool {
    bucket == Bucket::Today && !done && moved_at < today_start
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(title: &str, project: Option<&str>, mins: Option<i64>) -> Option<ParsedTask> {
        Some(ParsedTask { title: title.into(), project: project.map(Into::into), estimate_ms: mins.map(|m| m * MINUTE) })
    }

    #[test]
    fn parse_spec_examples() {
        assert_eq!(parse("Prep slides 28min #job-search"), p("Prep slides", Some("job-search"), Some(28)));
        assert_eq!(parse("#job-search Prep slides 1h"), p("Prep slides", Some("job-search"), Some(60)));
        assert_eq!(parse("Email campaign 2hr 15min"), p("Email campaign", None, Some(135)));
        assert_eq!(parse("Read 3 chapters"), p("Read 3 chapters", None, None));
        assert_eq!(parse("Call mom at 5pm"), p("Call mom at 5pm", None, None));
        assert_eq!(parse("Write 2 posts 45m"), p("Write 2 posts", None, Some(45)));
        assert_eq!(parse("Deep dive 0m"), p("Deep dive 0m", None, None));
        assert_eq!(parse("30m"), None);
        assert_eq!(parse("  #only-a-tag "), None);
    }

    #[test]
    fn parse_estimate_forms() {
        let est = |s: &str| parse(&format!("Task {s}")).and_then(|t| t.estimate_ms).map(|ms| ms / MINUTE);
        for (s, mins) in [
            ("28m", 28),
            ("28 min", 28),
            ("28mins", 28),
            ("28 minutes", 28),
            ("1h", 60),
            ("1hr", 60),
            ("1 hour", 60),
            ("2hrs", 120),
            ("2 hours", 120),
            ("1.5h", 90),
            ("1,5h", 90),
            ("1h30", 90),
            ("1h30m", 90),
            ("1h 30m", 90),
            ("2hr 15min", 135),
            ("1 hour 15 minutes", 75),
            ("90min", 90),
            ("45M", 45),
        ] {
            assert_eq!(est(s), Some(mins), "{s}");
        }
    }

    #[test]
    fn parse_leaves_non_estimates_in_the_title() {
        for s in ["Sprint 9h", "Plan 5 months", "Room 30", "Fix 1m30"] {
            assert_eq!(parse(s).unwrap().title, s);
        }
        // Only the trailing duration is taken; anything before it stays.
        let t = parse("Chapter 2 1h").unwrap();
        assert_eq!((t.title.as_str(), t.estimate_ms), ("Chapter 2", Some(60 * MINUTE)));
        let t = parse("Fix 30m 1h").unwrap();
        assert_eq!((t.title.as_str(), t.estimate_ms), ("Fix 30m", Some(60 * MINUTE)));
        assert_eq!(parse("Long haul 8h").unwrap().estimate_ms, Some(MAX_ESTIMATE_MS));
        assert_eq!(parse("Long haul 8h 1m").unwrap().title, "Long haul 8h 1m");
    }

    #[test]
    fn parse_collapses_whitespace_and_limits_the_title() {
        assert_eq!(parse("  Prep   slides\t25m ").unwrap().title, "Prep slides");
        let long = format!("{} 25m", "x".repeat(200));
        let t = parse(&long).unwrap();
        assert_eq!((t.title.chars().count(), t.estimate_ms), (MAX_TITLE, Some(25 * MINUTE)));
    }

    #[test]
    fn positions_fit_between_neighbours() {
        assert_eq!(position_between(None, None), Some(1.0));
        assert_eq!(position_between(Some(3.0), None), Some(4.0));
        assert_eq!(position_between(None, Some(3.0)), Some(2.0));
        assert_eq!(position_between(Some(1.0), Some(2.0)), Some(1.5));
        assert_eq!(position_between(Some(1.0), Some(1.0 + 1e-7)), None);
    }

    #[test]
    fn leftovers_are_open_today_tasks_from_earlier_days() {
        assert!(is_leftover(Bucket::Today, false, 99, 100));
        assert!(!is_leftover(Bucket::Today, false, 100, 100));
        assert!(!is_leftover(Bucket::Today, true, 99, 100));
        assert!(!is_leftover(Bucket::Week, false, 99, 100));
    }
}
