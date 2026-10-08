//! The weekly summary email: what goes in it (rendering is pure and
//! testable), when it's due, and how it's sent (Gmail SMTP with an app
//! password kept in the macOS Keychain).
//!
//! Privacy: this is the only place Chronos sends data off the machine, and
//! only when the user turns it on. Task names are never included.

use crate::analytics::PeriodStats;
use crate::insights::{Insight, Score, Suggestion};
use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};

pub const KEYCHAIN_SERVICE: &str = "Chronos weekly email";
const SMTP_HOST: &str = "smtp.gmail.com";
const MIN: i64 = 60_000;

/// Everything the email needs, gathered by the engine.
pub struct ReportData {
    pub period: PeriodStats,
    pub score: Option<Score>,
    pub prev_score: Option<Score>,
    pub mover: Option<(String, f64)>,
    pub insights: Vec<Insight>,
    pub suggestion: Option<Suggestion>,
    pub goal_ms: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Email {
    pub subject: String,
    pub html: String,
    pub text: String,
}

/// Persisted in kv so a week is reported exactly once.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReportStatus {
    /// Monday (YYYY-MM-DD) of the last week that was emailed or skipped.
    pub last_week: Option<String>,
    pub last_sent_at: Option<i64>,
    pub last_attempt_at: Option<i64>,
    pub last_error: Option<String>,
}

// ── schedule ───────────────────────────────────────────────────────────────

/// If the weekly send time (weekday `day` 0=Mon, `hour`) has passed in the
/// week containing `today`/`hour_now`, the Monday of the week to report
/// (the previous full week). Otherwise None.
pub fn due_week(today: NaiveDate, hour_now: u32, day: u32, hour: u32) -> Option<NaiveDate> {
    let monday = today - Duration::days(today.weekday().num_days_from_monday() as i64);
    let wd = today.weekday().num_days_from_monday();
    let past = wd > day || (wd == day && hour_now >= hour);
    past.then(|| monday - Duration::days(7))
}

// ── rendering ──────────────────────────────────────────────────────────────

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn hm(ms: i64) -> String {
    let m = (ms.max(0) + MIN / 2) / MIN;
    match (m / 60, m % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// "↑ 14%" / "↓ 9%" / "" (no comparison). For distractions, lower is better.
fn delta(now: f64, before: f64, lower_is_better: bool) -> (String, &'static str) {
    if before <= 0.0 {
        return (String::new(), "#a68d70");
    }
    let d = (now - before) / before;
    if d.abs() < 0.01 {
        return ("same as before".into(), "#a68d70");
    }
    let good = if lower_is_better { d < 0.0 } else { d > 0.0 };
    (
        format!("{} {}%", if d > 0.0 { "↑" } else { "↓" }, (d.abs() * 100.0).round()),
        if good { "#3f8a5a" } else { "#b5562a" },
    )
}

/// `date` formats a local-midnight timestamp, e.g. "Mon 29 Sep".
pub fn render(d: &ReportData, date: &dyn Fn(i64, &str) -> String) -> Email {
    let p = &d.period;
    let t = &p.totals;
    let prev = &p.previous;
    let range = format!("{} – {}", date(p.from, "%-d %b"), date(p.to - 24 * 60 * MIN, "%-d %b"));
    let score_txt = d.score.as_ref().map(|s| format!("{}/100", s.value));
    let (focus_delta, _) = delta(t.focus_ms as f64, prev.focus_ms as f64, false);

    let subject = match &score_txt {
        Some(s) => format!(
            "Chronos weekly: {s} · {} focused{} · {range}",
            hm(t.focus_ms),
            if focus_delta.starts_with('↑') || focus_delta.starts_with('↓') { format!(" ({focus_delta})") } else { String::new() }
        ),
        None => format!("Chronos weekly: {} focused · {range}", hm(t.focus_ms)),
    };

    // Rows: label, value, delta(text, colour)
    let rows: Vec<(&str, String, (String, &str))> = vec![
        ("Focus time", hm(t.focus_ms), delta(t.focus_ms as f64, prev.focus_ms as f64, false)),
        ("Average per day", hm(t.avg_daily_focus_ms), delta(t.avg_daily_focus_ms as f64, prev.avg_daily_focus_ms as f64, false)),
        (
            "Sessions completed",
            format!("{} of {} ({}%)", t.completed, t.focus_sessions, (t.completion_rate * 100.0).round()),
            delta(t.completed as f64, prev.completed as f64, false),
        ),
        ("Average session", hm(t.avg_session_ms), delta(t.avg_session_ms as f64, prev.avg_session_ms as f64, false)),
        ("Distractions", t.distractions.to_string(), delta(t.distractions as f64, prev.distractions as f64, true)),
        ("Days with a finished session", format!("{} of {}", t.active_days, t.elapsed_days), (String::new(), "")),
        (
            "Current streak",
            format!("{} day{} (best {})", p.streak.current, if p.streak.current == 1 { "" } else { "s" }, p.streak.best),
            (String::new(), ""),
        ),
    ];

    // ── HTML (inline styles only: that's what email clients render) ──
    let ink = "#3b2717";
    let muted = "#8a7058";
    let accent = "#d4672a";
    let mut h = String::new();
    h += &format!(
        r#"<!doctype html><html><body style="margin:0;padding:0;background:#f3ebda;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#f3ebda;padding:24px 0;font-family:-apple-system,'Avenir Next',Helvetica,Arial,sans-serif;color:{ink};">
<tr><td align="center">
<table role="presentation" width="560" cellpadding="0" cellspacing="0" style="max-width:560px;width:100%;">
<tr><td style="padding:0 20px 14px;font-size:12px;letter-spacing:3px;font-weight:800;color:{accent};">CHRONOS · WEEKLY</td></tr>
<tr><td style="padding:0 20px 18px;font-size:14px;color:{muted};">Your week of {range}</td></tr>"#
    );

    // Score card
    h += r#"<tr><td style="padding:0 12px 14px;"><table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#fbf6ea;border:1px solid #ddcfb2;border-radius:14px;"><tr><td style="padding:18px 20px;">"#;
    match &d.score {
        Some(s) => {
            let (sd, sc) = d.prev_score.as_ref().map(|p| delta(s.value as f64, p.value as f64, false)).unwrap_or_default();
            h += &format!(
                r#"<div style="font-size:12px;font-weight:800;letter-spacing:1.6px;color:{muted};">WEEKLY SCORE</div>
<div style="font-size:48px;font-weight:700;line-height:1.1;margin:6px 0 2px;">{}<span style="font-size:16px;color:{muted};font-weight:600;">/100</span></div>"#,
                s.value
            );
            if !sd.is_empty() {
                h += &format!(r#"<div style="font-size:14px;font-weight:700;color:{sc};">{} vs the week before</div>"#, esc(&sd));
            }
            if let Some((label, pts)) = &d.mover {
                h += &format!(
                    r#"<div style="font-size:13px;color:{muted};margin-top:2px;">Mostly from {} ({}{} pts)</div>"#,
                    esc(&label.to_lowercase()),
                    if *pts > 0.0 { "+" } else { "" },
                    pts.round()
                );
            }
            h += r#"<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="margin-top:14px;">"#;
            for part in &s.parts {
                let pct = (part.points / part.max * 100.0).round().clamp(0.0, 100.0);
                h += &format!(
                    r#"<tr><td style="font-size:13px;padding:4px 0;width:150px;">{}</td>
<td style="padding:4px 8px;"><table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#ebe0c9;border-radius:4px;"><tr><td width="{pct}%" style="background:{accent};height:8px;border-radius:4px;font-size:0;line-height:0;">&nbsp;</td><td style="font-size:0;line-height:0;">&nbsp;</td></tr></table></td>
<td style="font-size:13px;font-weight:700;text-align:right;width:44px;">{}<span style="color:{muted};font-weight:500;">/{}</span></td></tr>
<tr><td></td><td colspan="2" style="font-size:11.5px;color:{muted};padding:0 8px 4px;">{}</td></tr>"#,
                    esc(part.label),
                    part.points.round(),
                    part.max.round(),
                    esc(&part.detail)
                );
            }
            h += "</table>";
        }
        None => h += &format!(r#"<div style="font-size:14px;color:{muted};">No score this week.</div>"#),
    }
    h += "</td></tr></table></td></tr>";

    // Numbers
    h += r#"<tr><td style="padding:0 12px 14px;"><table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#fbf6ea;border:1px solid #ddcfb2;border-radius:14px;"><tr><td style="padding:14px 20px;">"#;
    h += &format!(r#"<div style="font-size:12px;font-weight:800;letter-spacing:1.6px;color:{muted};margin-bottom:6px;">THE NUMBERS</div><table role="presentation" width="100%" cellpadding="0" cellspacing="0">"#);
    for (label, value, (dt, dc)) in &rows {
        h += &format!(
            r#"<tr><td style="font-size:14px;padding:6px 0;border-top:1px solid #ece1cb;">{}</td><td style="font-size:14px;font-weight:700;text-align:right;padding:6px 0;border-top:1px solid #ece1cb;">{}</td><td style="font-size:12px;font-weight:700;color:{dc};text-align:right;padding:6px 0 6px 10px;border-top:1px solid #ece1cb;white-space:nowrap;width:70px;">{}</td></tr>"#,
            esc(label),
            esc(value),
            esc(dt)
        );
    }
    h += "</table></td></tr></table></td></tr>";

    // Days
    let max_day = p.days.iter().map(|x| x.focus_ms).max().unwrap_or(0).max(d.goal_ms).max(1);
    h += r#"<tr><td style="padding:0 12px 14px;"><table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#fbf6ea;border:1px solid #ddcfb2;border-radius:14px;"><tr><td style="padding:14px 20px;">"#;
    h += &format!(r#"<div style="font-size:12px;font-weight:800;letter-spacing:1.6px;color:{muted};margin-bottom:6px;">FOCUS PER DAY</div><table role="presentation" width="100%" cellpadding="0" cellspacing="0">"#);
    for day in &p.days {
        let pct = (day.focus_ms as f64 / max_day as f64 * 100.0).round().clamp(0.0, 100.0);
        let bar = if day.focus_ms > 0 {
            format!(r#"<table role="presentation" width="{pct}%" cellpadding="0" cellspacing="0"><tr><td style="background:{accent};height:12px;border-radius:3px;font-size:0;line-height:0;">&nbsp;</td></tr></table>"#)
        } else {
            String::new()
        };
        h += &format!(
            r#"<tr><td style="font-size:13px;padding:4px 0;width:70px;color:{muted};">{}</td><td style="padding:4px 8px;">{bar}</td><td style="font-size:13px;text-align:right;width:64px;">{}</td></tr>"#,
            esc(&date(day.start, "%a %-d")),
            if day.focus_ms > 0 { hm(day.focus_ms) } else { "–".into() }
        );
    }
    h += "</table></td></tr></table></td></tr>";

    // Projects + insights + suggestion
    let projects: Vec<String> = p
        .projects
        .iter()
        .take(5)
        .map(|n| format!("{} · {}", if n.name.is_empty() { "No project".into() } else { format!("#{}", n.name) }, hm(n.value)))
        .collect();
    let mut notes: Vec<(String, String)> = Vec::new();
    // Only worth a section if something was actually tagged.
    let tagged = p.projects.iter().any(|n| !n.name.is_empty());
    if tagged {
        let lines: Vec<String> = projects.iter().map(|p| esc(p)).collect();
        notes.push(("Where the time went".into(), lines.join("<br>")));
    }
    for i in d.insights.iter().take(3) {
        notes.push((esc(&i.text), esc(&i.evidence)));
    }
    if !notes.is_empty() {
        h += r#"<tr><td style="padding:0 12px 14px;"><table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#fbf6ea;border:1px solid #ddcfb2;border-radius:14px;"><tr><td style="padding:14px 20px;">"#;
        h += &format!(r#"<div style="font-size:12px;font-weight:800;letter-spacing:1.6px;color:{muted};margin-bottom:6px;">WHAT YOUR DATA SAYS</div>"#);
        for (title, body) in &notes {
            h += &format!(
                r#"<div style="padding:8px 0;border-top:1px solid #ece1cb;"><div style="font-size:14px;">{title}</div><div style="font-size:12px;color:{muted};margin-top:2px;">{body}</div></div>"#
            );
        }
        h += "</td></tr></table></td></tr>";
    }
    if let Some(s) = &d.suggestion {
        h += &format!(
            r#"<tr><td style="padding:0 12px 14px;"><table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#fbf6ea;border:1px solid {accent};border-radius:14px;"><tr><td style="padding:14px 20px;"><div style="font-size:12px;font-weight:800;letter-spacing:1.6px;color:{accent};margin-bottom:6px;">ONE THING TO TRY</div><div style="font-size:14px;">{}</div><div style="font-size:12px;color:{muted};margin-top:4px;">{} · Open Chronos → Insights to apply it.</div></td></tr></table></td></tr>"#,
            esc(&s.text),
            esc(&s.evidence)
        );
    }
    h += &format!(
        r#"<tr><td style="padding:6px 20px 0;font-size:11.5px;color:{muted};">Sent by Chronos from your Mac. Task names are never included. Turn this off in Chronos → Settings → Weekly email.</td></tr>
</table></td></tr></table></body></html>"#
    );

    // ── plain text alternative ──
    let mut x = format!("CHRONOS · WEEKLY\nYour week of {range}\n\n");
    if let Some(s) = &d.score {
        x += &format!("Weekly score: {}/100", s.value);
        if let Some(p) = &d.prev_score {
            let (sd, _) = delta(s.value as f64, p.value as f64, false);
            if !sd.is_empty() {
                x += &format!(" ({sd} vs the week before)");
            }
        }
        x += "\n";
        for part in &s.parts {
            x += &format!("  {}: {}/{} ({})\n", part.label, part.points.round(), part.max.round(), part.detail);
        }
        x += "\n";
    }
    for (label, value, (dt, _)) in &rows {
        x += &format!("{label}: {value}{}\n", if dt.is_empty() { String::new() } else { format!(" ({dt})") });
    }
    x += "\nFocus per day:\n";
    for day in &p.days {
        x += &format!("  {}: {}\n", date(day.start, "%a %-d"), hm(day.focus_ms));
    }
    if tagged {
        x += &format!("\nWhere the time went:\n  {}\n", projects.join("\n  "));
    }
    for i in d.insights.iter().take(3) {
        x += &format!("\n• {} ({})", i.text, i.evidence);
    }
    if let Some(s) = &d.suggestion {
        x += &format!("\n\nOne thing to try: {}", s.text);
    }
    x += "\n\n— Sent by Chronos from your Mac. Turn this off in Settings → Weekly email.\n";

    Email { subject, html: h, text: x }
}

// ── sending ────────────────────────────────────────────────────────────────

pub fn keychain_get(account: &str) -> Option<String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, account).ok()?.get_password().ok()
}

pub fn keychain_set(account: &str, password: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, account).map_err(|e| e.to_string())?;
    if password.is_empty() {
        let _ = entry.delete_credential();
        return Ok(());
    }
    entry.set_password(password).map_err(|e| e.to_string())
}

/// Send through Gmail (STARTTLS on 587) with an app password.
pub fn send_gmail(from: &str, app_password: &str, to: &str, email: &Email) -> Result<(), String> {
    use lettre::message::{header::ContentType, Mailbox, MultiPart, SinglePart};
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{Message, SmtpTransport, Transport};

    let from_box: Mailbox = format!("Chronos <{from}>").parse().map_err(|_| format!("“{from}” isn't a valid sender address."))?;
    let to_box: Mailbox = to.parse().map_err(|_| format!("“{to}” isn't a valid email address."))?;
    let msg = Message::builder()
        .from(from_box)
        .to(to_box)
        .subject(&email.subject)
        .multipart(
            MultiPart::alternative()
                .singlepart(SinglePart::builder().header(ContentType::TEXT_PLAIN).body(email.text.clone()))
                .singlepart(SinglePart::builder().header(ContentType::TEXT_HTML).body(email.html.clone())),
        )
        .map_err(|e| e.to_string())?;
    // Google shows app passwords as "abcd efgh ijkl mnop".
    let password: String = app_password.chars().filter(|c| !c.is_whitespace()).collect();
    let mailer = SmtpTransport::starttls_relay(SMTP_HOST)
        .map_err(|e| e.to_string())?
        .credentials(Credentials::new(from.to_string(), password))
        .timeout(Some(std::time::Duration::from_secs(20)))
        .build();
    mailer.send(&msg).map(|_| ()).map_err(|e| {
        let s = e.to_string();
        if s.contains("535") || s.to_lowercase().contains("credentials") {
            "Gmail rejected the login. Check the Gmail address and create a new app password.".into()
        } else {
            format!("Couldn't send: {s}")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::{DayAgg, Named, Streak, Totals};
    use crate::insights::ScorePart;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn due_week_after_the_send_time_only() {
        // Thursday 8 Oct 2026; send Mondays 9:00 → last week (Mon 28 Sep) is due.
        assert_eq!(due_week(d(2026, 10, 8), 15, 0, 9), Some(d(2026, 9, 28)));
        // Monday 8:00, before 9:00 → not yet.
        assert_eq!(due_week(d(2026, 10, 5), 8, 0, 9), None);
        assert_eq!(due_week(d(2026, 10, 5), 9, 0, 9), Some(d(2026, 9, 28)));
        // Sending on Fridays; it's Thursday → not yet.
        assert_eq!(due_week(d(2026, 10, 8), 23, 4, 9), None);
    }

    fn data() -> ReportData {
        const DAY: i64 = 24 * 60 * MIN;
        let days: Vec<DayAgg> = (0..7)
            .map(|i| DayAgg { start: i * DAY, focus_ms: [90, 125, 0, 60, 45, 0, 30][i as usize] * MIN, completed: 2, ..Default::default() })
            .collect();
        let totals = Totals {
            focus_ms: 350 * MIN,
            focus_sessions: 14,
            completed: 12,
            abandoned: 2,
            distractions: 6,
            completion_rate: 12.0 / 14.0,
            avg_session_ms: 25 * MIN,
            avg_daily_focus_ms: 50 * MIN,
            active_days: 5,
            elapsed_days: 7,
            ..Default::default()
        };
        let previous = Totals { focus_ms: 300 * MIN, distractions: 8, completed: 10, ..totals.clone() };
        ReportData {
            period: PeriodStats {
                from: 0,
                to: 7 * DAY,
                days,
                totals,
                previous,
                projects: vec![Named { name: "job<search>".into(), value: 200 * MIN }],
                streak: Streak { current: 3, best: 5, today_done: true },
                ..Default::default()
            },
            score: Some(Score {
                value: 74,
                parts: vec![ScorePart { key: "focus", label: "Focus time", points: 18.0, max: 25.0, detail: "50m/day of 3h goal".into() }],
            }),
            prev_score: Some(Score { value: 66, parts: vec![] }),
            mover: Some(("Consistency".into(), 6.0)),
            insights: vec![Insight { id: "x", text: "Distractions peak 2–4 PM.".into(), evidence: "41% of 22".into() }],
            suggestion: None,
            goal_ms: 180 * MIN,
        }
    }

    #[test]
    fn renders_subject_numbers_and_escapes() {
        let e = render(&data(), &|t, f| if f == "%-d %b" { format!("{}", t / (24 * 60 * MIN)) } else { "Day".into() });
        assert_eq!(e.subject, "Chronos weekly: 74/100 · 5h 50m focused (↑ 17%) · 0 – 6");
        assert!(e.html.contains(">74<"));
        assert!(e.html.contains("↑ 12% vs the week before"));
        assert!(e.html.contains("#job&lt;search&gt;"), "project names are escaped");
        assert!(!e.html.contains("job<search>"));
        assert!(e.text.contains("Distractions: 6 (↓ 25%)"));
        assert!(e.text.contains("Weekly score: 74/100"));
    }
}
