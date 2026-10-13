// The small popup next to the clock: "what distracted you?" and the
// end-of-session reflection. Always optional: it closes itself, and Esc or
// Skip dismisses it without recording anything.

import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { fmtTime } from "../format";
import { clockStyleClass, useSettings } from "../hooks";
import { DISTRACTION_CATEGORIES, type Quick as QuickData } from "../types";
import { AddTask } from "./AddTask";
import "./quick.css";

const RATINGS = [
  { value: 1, face: "😫", label: "Rough" },
  { value: 2, face: "😐", label: "Meh" },
  { value: 3, face: "🙂", label: "Good" },
  { value: 4, face: "🔥", label: "Great" },
];
const GOALS = [
  { value: "yes", label: "Yes" },
  { value: "partial", label: "Partly" },
  { value: "no", label: "No" },
];

export function Quick() {
  const [q, setQ] = useState<QuickData | null>(null);
  const [settings] = useSettings();
  useEffect(() => {
    api.quickState().then(setQ);
    return api.onQuick(setQ);
  }, []);
  if (!q) return null;
  if (q.kind === "addTask") return <AddTask />;
  return (
    <div className={`quick-root ${clockStyleClass(settings?.style)}`}>
      {q.kind === "distraction" ? <Distraction key={q.id} q={q} /> : <Reflection key={q.sessionId} q={q} />}
    </div>
  );
}

/** Close after `ms`, but never while the pointer is over the card. */
function useAutoClose(ms: number) {
  const hovering = useRef(false);
  useEffect(() => {
    let left = ms;
    const id = window.setInterval(() => {
      if (!hovering.current) left -= 250;
      if (left <= 0) api.quickClose();
    }, 250);
    return () => clearInterval(id);
  }, [ms]);
  return {
    onMouseEnter: () => (hovering.current = true),
    onMouseLeave: () => (hovering.current = false),
  };
}

function useEscape(fn: () => void) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && fn();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
}

function Distraction({ q }: { q: Extract<QuickData, { kind: "distraction" }> }) {
  const [picked, setPicked] = useState<string | null>(null);
  const hover = useAutoClose(8000);
  const close = () => api.quickClose();
  useEscape(close);

  const pick = (id: string) => {
    setPicked(id);
    api.setDistractionCategory(q.id, id);
    window.setTimeout(close, 450);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const c = DISTRACTION_CATEGORIES.find((c) => c.key === e.key);
      if (c) pick(c.id);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="quick-card" {...hover}>
      <header>
        <span className="dot" />
        <b>Distraction #{q.countToday} logged</b>
        <span className="meta">
          {q.sessionNumber ? `session ${q.sessionNumber} · ` : ""}
          {fmtTime(Date.now())}
        </span>
        <button className="x" onClick={close} title="Close (Esc)">×</button>
      </header>
      <div className="ask">{picked ? "Noted. Back to it." : "What pulled you away? (optional)"}</div>
      <div className="cats">
        {DISTRACTION_CATEGORIES.map((c) => (
          <button key={c.id} className={picked === c.id ? "on" : ""} onClick={() => pick(c.id)} title={`${c.label} (press ${c.key})`}>
            <kbd>{c.key}</kbd>
            {c.short}
          </button>
        ))}
      </div>
    </div>
  );
}

function Reflection({ q }: { q: Extract<QuickData, { kind: "reflection" }> }) {
  const [rating, setRating] = useState<number | null>(null);
  const [goal, setGoal] = useState<string | null>(null);
  const hover = useAutoClose(45_000);
  const close = () => api.quickClose();
  useEscape(close);
  const done = rating !== null && (goal !== null || !q.task);

  useEffect(() => {
    if (done) {
      const t = window.setTimeout(close, 700);
      return () => clearTimeout(t);
    }
  }, [done]);

  return (
    <div className="quick-card" {...hover}>
      <header>
        <span className="check">✓</span>
        <b>Session complete</b>
        <span className="meta">
          {q.minutes} min focused{q.project ? ` · #${q.project}` : ""}
        </span>
        <button className="x" onClick={close} title="Skip (Esc)">×</button>
      </header>
      <div className="ask">{done ? "Thanks. Enjoy the break." : "How productive was this session?"}</div>
      <div className="faces">
        {RATINGS.map((r) => (
          <button
            key={r.value}
            className={rating === r.value ? "on" : ""}
            title={r.label}
            onClick={() => {
              setRating(r.value);
              api.rateSession(q.sessionId, r.value, null);
            }}
          >
            <span>{r.face}</span>
            <small>{r.label}</small>
          </button>
        ))}
      </div>
      {q.task && (
        <div className="goal">
          <span className="goal-q" title={q.task}>
            Did you finish “{q.task}”?
          </span>
          {GOALS.map((g) => (
            <button
              key={g.value}
              className={goal === g.value ? "on" : ""}
              onClick={() => {
                setGoal(g.value);
                api.rateSession(q.sessionId, null, g.value);
              }}
            >
              {g.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
