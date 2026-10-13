// Today tab: how the day is going. Focus against the goal, what is running
// now, a few key numbers, the day's timeline, where the time went and the
// session log (with distractions under the session they interrupted).

import { useEffect, useState } from "react";
import { api } from "../api";
import { fmtClock, fmtDuration, fmtTime } from "../format";
import { remainingOf, useSecondTick, useSnapshot } from "../hooks";
import type { DistractionRow, Named, SessionRow, Snapshot, TodayStats } from "../types";
import { DISTRACTION_CATEGORIES, PHASE_LABEL } from "../types";
import { ratingFace } from "./History";
import { projectColor } from "./planModel";
import "./today.css";

export function Today() {
  const snap = useSnapshot();
  const now = useSecondTick(snap);
  const stats = useTodayStats(snap);

  if (!snap || !stats) return <div className="empty">Loading…</div>;

  const date = new Date(now).toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" });
  const sessions = stats.sessions;
  const focus = sessions.filter((s) => s.kind === "focus");
  const extras = [
    stats.interrupted > 0 && `${stats.interrupted} paused`,
    stats.abandoned > 0 && `${stats.abandoned} ended early`,
    focus.length > 0 && `longest ${fmtDuration(stats.longestFocusMs)}`,
    stats.breakMs > 0 && `${fmtDuration(stats.breakMs)} of breaks`,
    stats.currentStreak > 1 && `${stats.currentStreak} done in a row`,
    stats.firstStart && stats.lastEnd && `${fmtTime(stats.firstStart)} – ${fmtTime(stats.lastEnd)}`,
  ].filter((e): e is string => !!e);

  return (
    <div className="today">
      <header className="td-head">
        <div className="td-title">
          <span className="td-date">{date}</span>
          <h1>
            {fmtDuration(snap.focusMsToday)} <span>focused</span>
          </h1>
        </div>
        <div className="td-chips">
          <span className="td-chip" title={`Best: ${stats.streak.best} days`}>
            {stats.streak.current > 0 ? `🔥 ${stats.streak.current}-day streak` : "No streak yet"}
          </span>
          {stats.weekScore !== null && (
            <a className="td-chip link-chip" href="#insights" title="See what makes up the score">
              Week score {stats.weekScore} ›
            </a>
          )}
        </div>
      </header>

      <Goal doneMs={snap.focusMsToday} goalMs={snap.goalMs} streak={stats.streak} />

      {snap.run !== "idle" && <NowStrip snap={snap} now={now} />}

      <section className="card td-figures">
        <div className="td-figs">
          <Figure value={stats.focusSessions} label={stats.focusSessions === 1 ? "session" : "sessions"} />
          <Figure value={stats.completed} label="completed" tone={stats.completed > 0 ? "good" : undefined} />
          <Figure value={stats.focusSessions ? fmtDuration(stats.avgFocusMs) : "—"} label="avg session" />
          <Figure value={stats.distractions} label={stats.distractions === 1 ? "distraction" : "distractions"} />
        </div>
        {extras.length > 0 && (
          <div className="td-extras">
            <div>
              <div>
                {extras.map((e) => (
                  <span key={e}>{e}</span>
                ))}
              </div>
            </div>
          </div>
        )}
      </section>

      <section className="card">
        <div className="td-card-head">
          <h3>Timeline</h3>
          <div className="td-legend">
            <span><i className="tl-key focus" />Focus</span>
            <span><i className="tl-key short_break" />Break</span>
            <span><i className="tl-key abandoned" />Ended early</span>
            {stats.distractionList.length > 0 && <span><i className="tl-key distraction" />Distraction</span>}
          </div>
        </div>
        <Timeline sessions={sessions} distractions={stats.distractionList} snap={snap} now={now} />
      </section>

      <section className="card">
        <h3>Projects</h3>
        <Projects rows={stats.projects} />
      </section>

      <section className="card td-log-card">
        <div className="td-card-head">
          <h3>Sessions</h3>
          {focus.length > 0 && <span className="td-card-note">{fmtDuration(stats.focusMs)} of focus in {focus.length}</span>}
        </div>
        <SessionLog sessions={sessions} distractions={stats.distractionList} />
      </section>
    </div>
  );
}

/** Refetch when a block finishes, the day rolls over, or every minute. */
function useTodayStats(snap: Snapshot | null): TodayStats | null {
  const [stats, setStats] = useState<TodayStats | null>(null);
  const key = snap ? `${snap.completedToday}|${snap.phase}|${snap.run}|${snap.distractionsToday}|${new Date().toDateString()}` : "";
  useEffect(() => {
    api.todayStats().then(setStats);
  }, [key]);
  useEffect(() => {
    const id = window.setInterval(() => api.todayStats().then(setStats), 60_000);
    const off = api.onStats(() => api.todayStats().then(setStats));
    return () => {
      clearInterval(id);
      off();
    };
  }, []);
  return stats;
}

function Goal({ doneMs, goalMs, streak }: { doneMs: number; goalMs: number; streak: TodayStats["streak"] }) {
  const keepStreak = streak.current > 0 && !streak.todayDone && "Finish a session today to keep your streak.";
  if (goalMs <= 0) return keepStreak ? <div className="td-goal-text">{keepStreak}</div> : null;
  const pct = Math.min(1, doneMs / goalMs);
  const left = goalMs - doneMs;
  return (
    <div className="td-goal">
      <div className="td-goal-text">
        <span>
          <strong>{Math.round(pct * 100)}%</strong> of your {fmtDuration(goalMs)} goal
        </span>
        <span className={left <= 0 ? "reached" : ""}>
          {left > 0 ? `${fmtDuration(left)} to go` : left < -60_000 ? `Goal reached · ${fmtDuration(-left)} over` : "Goal reached"}
        </span>
      </div>
      <div className="td-bar" role="progressbar" aria-valuenow={Math.round(pct * 100)} aria-valuemin={0} aria-valuemax={100}>
        <i style={{ width: `${pct * 100}%` }} />
      </div>
      {keepStreak && <div className="td-goal-note">{keepStreak}</div>}
    </div>
  );
}

function NowStrip({ snap, now }: { snap: Snapshot; now: number }) {
  const running = snap.run === "running";
  const left = fmtClock(remainingOf(snap, now));
  const onTask = snap.phase === "focus" && snap.task;
  return (
    <section className={`td-now phase-${snap.phase}${running ? " running" : ""}`}>
      <span className="td-now-dot" aria-hidden />
      <div className="td-now-main">
        <span className="td-now-time">
          {running ? PHASE_LABEL[snap.phase] : "Paused"} · <b>{left}</b> left
        </span>
        {onTask && (
          <span className="td-now-task">
            {snap.task}
            {snap.project && (
              <span className="td-proj">
                <i style={{ background: projectColor(snap.project) }} />#{snap.project}
              </span>
            )}
          </span>
        )}
      </div>
      <button className="ghost td-now-btn" onClick={() => api.act({ type: "toggle" })}>
        {running ? "Pause" : "Continue"}
      </button>
    </section>
  );
}

function Figure({ value, label, tone }: { value: string | number; label: string; tone?: "good" }) {
  return (
    <div className={`td-fig ${tone ?? ""}`}>
      <b>{value}</b>
      <span>{label}</span>
    </div>
  );
}

function Timeline({ sessions, distractions, snap, now }: { sessions: SessionRow[]; distractions: DistractionRow[]; snap: Snapshot; now: number }) {
  const day = new Date(now);
  day.setHours(0, 0, 0, 0);
  const d0 = day.getTime();
  const hourOf = (t: number) => (t - d0) / 3_600_000;
  const nowH = hourOf(now);
  const firstH = sessions.length ? hourOf(sessions[0].startedAt) : nowH;
  const lastH = Math.max(nowH, ...sessions.map((s) => hourOf(s.endedAt ?? s.startedAt)));
  const from = Math.max(0, Math.min(8, Math.floor(firstH)));
  const to = Math.min(24, Math.max(from + 6, Math.ceil(lastH)));
  const x = (h: number) => `${((h - from) / (to - from)) * 100}%`;
  const w = (h0: number, h1: number) => `${Math.max(0.5, ((h1 - h0) / (to - from)) * 100)}%`;

  const step = to - from > 12 ? 3 : 2;
  const hours = [];
  for (let h = from; h <= to; h += step) hours.push(h);

  return (
    <div className="tl">
      <div className="tl-track">
        {hours.slice(1, -1).map((h) => (
          <span key={h} className="tl-grid" style={{ left: x(h) }} />
        ))}
        {sessions.map((s) => (
          <span
            key={s.id}
            className={`tl-block ${s.kind} ${s.status}`}
            style={{ left: x(hourOf(s.startedAt)), width: w(hourOf(s.startedAt), hourOf(s.endedAt ?? s.startedAt)) }}
            title={`${PHASE_LABEL[s.kind]} · ${fmtTime(s.startedAt)} · ${fmtDuration(s.actualMs)}${s.status === "abandoned" ? " · ended early" : ""}${s.task ? ` · ${s.task}` : ""}`}
          />
        ))}
        {snap.run !== "idle" && <span className={`tl-block live ${snap.phase}`} style={{ left: x(nowH - 0.03), width: "4px" }} />}
        <span className="tl-now" style={{ left: x(nowH) }} />
      </div>
      <div className="tl-marks">
        {distractions.map((d) => (
          <span key={d.id} className="tl-dot" style={{ left: x(hourOf(d.at)) }} title={`${fmtTime(d.at)} · ${categoryLabel(d.category)}`} />
        ))}
      </div>
      <div className="tl-hours">
        {hours.map((h) => (
          <span key={h} style={{ left: x(h) }}>
            {new Date(d0 + h * 3_600_000).toLocaleTimeString([], { hour: "numeric" })}
          </span>
        ))}
      </div>
    </div>
  );
}

function Projects({ rows }: { rows: Named[] }) {
  if (rows.length === 0) return <div className="empty">Add #project to a task to see where the time goes.</div>;
  const total = rows.reduce((a, r) => a + r.value, 0);
  const max = Math.max(...rows.map((r) => r.value));
  return (
    <ul className="td-projects">
      {rows.slice(0, 6).map((r) => {
        const color = r.name ? projectColor(r.name) : "var(--ink-3)";
        return (
          <li key={r.name} title={`${Math.round((r.value / total) * 100)}% of today's focus`}>
            <span className="td-proj">
              <i style={{ background: color }} />
              {r.name ? `#${r.name}` : "No project"}
            </span>
            <span className="td-proj-track">
              <i style={{ width: `${(r.value / max) * 100}%`, background: color }} />
            </span>
            <span className="td-proj-value">{fmtDuration(r.value)}</span>
          </li>
        );
      })}
    </ul>
  );
}

const categoryLabel = (c: string | null) => DISTRACTION_CATEGORIES.find((x) => x.id === c)?.label ?? "Distraction";

function SessionLog({ sessions, distractions }: { sessions: SessionRow[]; distractions: DistractionRow[] }) {
  if (sessions.length === 0 && distractions.length === 0) {
    return <div className="empty">No sessions yet today. Press play on the clock when you're ready.</div>;
  }
  // Each distraction goes under the session it happened in; any outside a
  // session get a row of their own.
  const inside = (s: SessionRow, d: DistractionRow) => d.sessionId === s.id || (d.sessionId == null && d.at >= s.startedAt && d.at <= (s.endedAt ?? Infinity));
  const loose = distractions.filter((d) => !sessions.some((s) => inside(s, d)));
  const rows: ({ at: number } & ({ s: SessionRow } | { d: DistractionRow }))[] = [
    ...sessions.map((s) => ({ at: s.startedAt, s })),
    ...loose.map((d) => ({ at: d.at, d })),
  ].sort((a, b) => b.at - a.at);

  return (
    <ol className="td-log">
      {rows.map((r) => {
        if ("d" in r) {
          return (
            <li key={`d${r.d.id}`} className="td-row minor">
              <span className="td-row-time">{fmtTime(r.d.at)}</span>
              <span className="td-row-main">{categoryLabel(r.d.category)}</span>
            </li>
          );
        }
        const s = r.s;
        if (s.kind !== "focus") {
          return (
            <li key={s.id} className="td-row minor">
              <span className="td-row-time">{fmtTime(s.startedAt)}</span>
              <span className="td-row-main">
                <i className={`td-kind ${s.kind}`} />
                {PHASE_LABEL[s.kind]}
              </span>
              <span className="td-row-dur">{fmtDuration(s.actualMs)}</span>
            </li>
          );
        }
        const ds = distractions.filter((d) => inside(s, d));
        const meta = [
          s.pauseCount > 0 && `paused ${s.pauseCount}×`,
          ds.length === 1 && categoryLabel(ds[0].category),
          ds.length > 1 && `${ds.length} distractions`,
        ].filter(Boolean);
        const early = s.status === "abandoned";
        return (
          <li key={s.id} className={`td-row${early ? " early" : ""}`}>
            <span className="td-row-time">{fmtTime(s.startedAt)}</span>
            <span className="td-row-main">
              <span className="td-row-title">{s.task ?? "Focus"}</span>
              {(s.project || meta.length > 0 || early) && (
                <span className="td-row-meta">
                  {s.project && (
                    <span className="td-proj">
                      <i style={{ background: projectColor(s.project) }} />#{s.project}
                    </span>
                  )}
                  {early && <span className="td-early">ended early</span>}
                  {meta.length > 0 && <span>{meta.join(" · ")}</span>}
                </span>
              )}
            </span>
            <span className="td-row-dur">
              {fmtDuration(s.actualMs)}
              {s.rating ? (
                <span className="td-rating" title="Your rating">{ratingFace(s.rating)}</span>
              ) : (
                !early && <span className="td-tick" aria-label="completed">✓</span>
              )}
            </span>
          </li>
        );
      })}
    </ol>
  );
}
