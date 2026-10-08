import { useEffect, useState } from "react";
import { api } from "../api";
import { fmtClock, fmtDuration, fmtTime } from "../format";
import { remainingOf, useSecondTick, useSnapshot } from "../hooks";
import type { DayStats, SessionRow, Snapshot } from "../types";
import { PHASE_LABEL } from "../types";

export function Today() {
  const snap = useSnapshot();
  const now = useSecondTick(snap);
  const stats = useTodayStats(snap);

  if (!snap || !stats) return <div className="empty">Loading…</div>;

  // Finished sessions plus whatever is running right now.
  const liveFocus = snap.focusMsToday;
  const goal = snap.goalMs;
  const pct = goal > 0 ? Math.min(1, liveFocus / goal) : 0;
  const date = new Date().toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" });

  return (
    <div className="today">
      <section className="hero card">
        <div className="hero-top">
          <div>
            <div className="eyebrow">Today · {date}</div>
            <div className="hero-figure">{fmtDuration(liveFocus)}</div>
            <div className="hero-caption">focused{goal > 0 && <> of {fmtDuration(goal)} goal</>}</div>
          </div>
          <NowBadge snap={snap} now={now} />
        </div>
        {goal > 0 && <GoalBar pct={pct} />}
      </section>

      <section className="grid">
        <Stat label="Focus sessions" value={stats.focusSessions} />
        <Stat label="Completed" value={stats.completed} tone="good" />
        <Stat label="Interrupted" value={stats.interrupted} hint="paused at least once" />
        <Stat label="Abandoned" value={stats.abandoned} tone={stats.abandoned ? "warn" : undefined} hint="ended early" />
        <Stat label="Break time" value={fmtDuration(stats.breakMs)} />
        <Stat label="Avg session" value={stats.focusSessions ? fmtDuration(stats.avgFocusMs) : "—"} />
        <Stat label="Longest focus" value={stats.focusSessions ? fmtDuration(stats.longestFocusMs) : "—"} />
        <Stat label="Current streak" value={`${stats.currentStreak}`} hint={stats.currentStreak === 1 ? "session" : "sessions"} />
        <Stat label="First start" value={stats.firstStart ? fmtTime(stats.firstStart) : "—"} />
        <Stat label="Last finish" value={stats.lastEnd ? fmtTime(stats.lastEnd) : "—"} />
      </section>

      <section className="card">
        <h3>Timeline</h3>
        <Timeline sessions={stats.sessions} snap={snap} now={now} />
      </section>

      <section className="card">
        <h3>Sessions</h3>
        <SessionLog sessions={stats.sessions} />
      </section>
    </div>
  );
}

/** Refetch when a block finishes, the day rolls over, or every minute. */
function useTodayStats(snap: Snapshot | null): DayStats | null {
  const [stats, setStats] = useState<DayStats | null>(null);
  const key = snap ? `${snap.completedToday}|${snap.phase}|${snap.run}|${new Date().toDateString()}` : "";
  useEffect(() => {
    api.todayStats().then(setStats);
  }, [key]);
  useEffect(() => {
    const id = window.setInterval(() => api.todayStats().then(setStats), 60_000);
    return () => clearInterval(id);
  }, []);
  return stats;
}

function NowBadge({ snap, now }: { snap: Snapshot; now: number }) {
  const state = snap.run === "running" ? "running" : snap.run === "paused" ? "paused" : "ready";
  return (
    <div className={`now-badge phase-${snap.phase} ${state}`}>
      <span className="dot" />
      <div>
        <div className="now-phase">
          {PHASE_LABEL[snap.phase]} · {state}
        </div>
        <div className="now-time">{fmtClock(remainingOf(snap, now))}</div>
        {snap.task && snap.phase === "focus" && <div className="now-task">{snap.task}</div>}
      </div>
    </div>
  );
}

function GoalBar({ pct }: { pct: number }) {
  const cells = 24;
  const lit = Math.round(pct * cells);
  return (
    <div className="goalbar" role="progressbar" aria-valuenow={Math.round(pct * 100)} aria-valuemin={0} aria-valuemax={100}>
      {Array.from({ length: cells }, (_, i) => (
        <i key={i} className={i < lit ? "on" : ""} />
      ))}
      <span>{Math.round(pct * 100)}%</span>
    </div>
  );
}

function Stat({ label, value, hint, tone }: { label: string; value: string | number; hint?: string; tone?: "good" | "warn" }) {
  return (
    <div className={`stat ${tone ?? ""}`}>
      <div className="stat-value">{value}</div>
      <div className="stat-label">
        {label}
        {hint && <span> · {hint}</span>}
      </div>
    </div>
  );
}

function Timeline({ sessions, snap, now }: { sessions: SessionRow[]; snap: Snapshot; now: number }) {
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
  const w = (h0: number, h1: number) => `${Math.max(0.4, ((h1 - h0) / (to - from)) * 100)}%`;

  const hours = [];
  for (let h = from; h <= to; h += (to - from) > 12 ? 3 : 2) hours.push(h);

  return (
    <div className="timeline">
      <div className="track">
        {sessions.map((s) => (
          <span
            key={s.id}
            className={`block ${s.kind} ${s.status}`}
            style={{ left: x(hourOf(s.startedAt)), width: w(hourOf(s.startedAt), hourOf(s.endedAt ?? s.startedAt)) }}
            title={`${PHASE_LABEL[s.kind]} · ${fmtTime(s.startedAt)} · ${fmtDuration(s.actualMs)} · ${s.status}${s.task ? ` · ${s.task}` : ""}`}
          />
        ))}
        {snap.run !== "idle" && <span className={`block live ${snap.phase}`} style={{ left: x(nowH - 0.02), width: "3px" }} />}
        <span className="nowline" style={{ left: x(nowH) }} />
      </div>
      <div className="hours">
        {hours.map((h) => (
          <span key={h} style={{ left: x(h) }}>
            {new Date(d0 + h * 3_600_000).toLocaleTimeString([], { hour: "numeric" })}
          </span>
        ))}
      </div>
      <div className="legend">
        <span><i className="block focus completed" />Focus</span>
        <span><i className="block focus abandoned" />Ended early</span>
        <span><i className="block short_break completed" />Break</span>
      </div>
    </div>
  );
}

function SessionLog({ sessions }: { sessions: SessionRow[] }) {
  if (sessions.length === 0) {
    return <div className="empty">No sessions yet today. Press play on the clock when you're ready.</div>;
  }
  return (
    <ol className="log">
      {[...sessions].reverse().map((s) => (
        <li key={s.id} className={`${s.kind} ${s.status}`}>
          <span className="log-time">{fmtTime(s.startedAt)}</span>
          <span className={`log-kind ${s.kind}`}>{PHASE_LABEL[s.kind]}</span>
          <span className="log-task">{s.task ?? ""}</span>
          <span className="log-dur">
            {fmtDuration(s.actualMs)}
            {s.pauseCount > 0 && <em title={`Paused ${s.pauseCount}×`}> ⏸{s.pauseCount}</em>}
          </span>
          <span className={`log-status ${s.status}`}>{s.status === "completed" ? "✓" : "ended early"}</span>
        </li>
      ))}
    </ol>
  );
}
