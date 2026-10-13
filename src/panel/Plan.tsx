import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { fmtClock, fmtDuration } from "../format";
import { remainingOf, useSecondTick, useSettings, useSnapshot } from "../hooks";
import type { Bucket, ParsedTask, Snapshot, TaskRow } from "../types";
import { PanelColumns } from "./PlanColumns";
import { applyMove, COLUMN_LABEL, estimateLabel, openIn, projectColor, projectedLoad, spentOf, type Column, type Load } from "./planModel";
import { PlayIcon, type CardActions } from "./TaskCard";
import { stepBefore, useDragList } from "./useDragList";
import "./plan.css";

const COLUMN_KEY = "chronos.planColumn";
const DEST_CYCLE: Bucket[] = ["today", "week", "backlog"];

export function Plan() {
  const snap = useSnapshot();
  const now = useSecondTick(snap);
  const [settings] = useSettings();
  const [tasks, setTasks] = useTasks(snap);
  const [column, setColumnState] = useState<Column>(savedColumn);
  const [toast, setToast] = useState<Toast | null>(null);
  const refocus = useRef<number | null>(null);

  const setColumn = (c: Column) => {
    setColumnState(c);
    try {
      localStorage.setItem(COLUMN_KEY, c);
    } catch {
      // Storage off: the tab just opens on Today next time.
    }
  };

  // After a keyboard move the card's node is re-inserted; put focus back on it.
  useEffect(() => {
    if (refocus.current == null) return;
    document.querySelector<HTMLElement>(`[data-drag-id="${refocus.current}"]`)?.focus();
    refocus.current = null;
  }, [tasks]);

  const fail = (e: unknown) => setToast({ text: String(e) });
  const move = (id: number, bucket: Bucket, beforeId: number | null) => {
    setTasks((ts) => ts && applyMove(ts, id, bucket, beforeId));
    api.taskMove(id, bucket, beforeId).catch(fail);
  };
  const setDone = (t: TaskRow, done: boolean) => {
    setTasks((ts) => ts && ts.map((x) => (x.id === t.id ? { ...x, done, doneAt: done ? Date.now() : null } : x)));
    api.taskSetDone(t.id, done).catch(fail);
  };

  const dnd = useDragList((id, drop) => {
    const t = tasks?.find((x) => x.id === id);
    if (!t) return;
    if (drop.column === "done") setDone(t, true);
    else move(id, drop.column, drop.beforeId);
  });

  const actions: CardActions = {
    start: (t) => api.act({ type: "startTask", value: t.id }),
    setDone,
    move: (t, bucket, beforeId = null) => {
      move(t.id, bucket, beforeId);
      if (bucket !== column) setToast({ text: `Moved to ${COLUMN_LABEL[bucket]}` });
    },
    step: (t, delta) => {
      const ids = openIn(tasks ?? [], t.bucket).map((x) => x.id);
      const before = stepBefore(ids, t.id, delta);
      if (before === undefined) return;
      refocus.current = t.id;
      move(t.id, t.bucket, before);
    },
    remove: (t) => {
      setTasks((ts) => ts && ts.filter((x) => x.id !== t.id));
      api.taskDelete(t.id).then(
        () => setToast({ text: `Deleted “${t.title}”`, undo: true }),
        fail,
      );
    },
    rename: (t, text) =>
      api.taskUpdate(t.id, { title: text }).then(
        (row) => (setTasks((ts) => ts && ts.map((x) => (x.id === row.id ? row : x))), true),
        () => false,
      ),
  };

  if (!snap || !tasks) return <div className="empty">Loading…</div>;

  const todayTasks = openIn(tasks, "today");
  const load = projectedLoad(todayTasks, focusNow(snap, now), snap.goalMs, (t) => spentOf(t, snap, now));
  const viewing: Bucket = column === "done" ? "today" : column;

  return (
    <div className="plan">
      <div className="plan-title">
        <div>
          <h1>{COLUMN_LABEL[column]}</h1>
          <span className="plan-sub">{subtitle(column, tasks, now)}</span>
        </div>
      </div>

      {column === "today" && <LoadBar load={load} />}

      <QuickAdd viewing={viewing} onAdded={(row) => row.bucket !== column && setToast({ text: `Added to ${COLUMN_LABEL[row.bucket]}` })} />

      <PanelColumns tasks={tasks} column={column} setColumn={setColumn} snap={snap} now={now} actions={actions} dnd={dnd} />

      <span className="plan-spacer" />
      <Footer snap={snap} now={now} top={todayTasks[0] ?? null} focusMin={settings?.focusMin ?? 25} shortMin={settings?.shortMin ?? 5} />

      {toast && <ToastBar toast={toast} onClose={() => setToast(null)} />}
    </div>
  );
}

function savedColumn(): Column {
  try {
    const c = localStorage.getItem(COLUMN_KEY);
    return c === "week" || c === "backlog" || c === "done" ? c : "today";
  } catch {
    return "today";
  }
}

/** Open tasks plus the last week's finished ones, refetched on every change. */
function useTasks(snap: Snapshot | null) {
  const [tasks, setTasks] = useState<TaskRow[] | null>(null);
  const seq = useRef(0);
  const load = () => {
    const n = ++seq.current;
    api.tasksList(7).then((rows) => n === seq.current && setTasks(rows));
  };
  useEffect(() => {
    load();
    return api.onTasks(load);
  }, []);
  // A block ending moves its credit from the timer into the database.
  const key = snap && `${snap.phase}|${snap.run}|${snap.taskId}|${snap.completedToday}`;
  useEffect(() => {
    if (key) load();
  }, [key]);
  return [tasks, setTasks] as const;
}

/** Focus time today, counting the running block up to `now`. */
function focusNow(snap: Snapshot, now: number): number {
  const running = snap.phase === "focus" && snap.run === "running";
  return snap.focusMsToday + (running ? Math.max(0, now - snap.now) : 0);
}

function subtitle(column: Column, tasks: TaskRow[], now: number): string {
  if (column === "today") return new Date(now).toLocaleDateString([], { weekday: "long", day: "numeric", month: "long" });
  if (column === "done") return "Finished today, newest first";
  const open = openIn(tasks, column);
  const est = open.reduce((a, t) => a + (t.estimateMs ?? 0), 0);
  const n = `${open.length} ${open.length === 1 ? "task" : "tasks"}`;
  return est > 0 ? `${n} · ${fmtDuration(est)} estimated` : n;
}

function LoadBar({ load }: { load: Load }) {
  const { doneMs, toGoMs, projectedMs, goalMs, unestimated } = load;
  const scale = Math.max(goalMs, projectedMs, 1);
  const overMs = goalMs > 0 ? projectedMs - goalMs : 0;
  const pct = (ms: number) => `${(ms / scale) * 100}%`;
  const right =
    goalMs === 0 ? null
    : overMs > 0 ? `${fmtDuration(overMs)} over`
    : overMs < 0 ? `${fmtDuration(-overMs)} to spare`
    : "right on your goal";
  return (
    <div className="load" title={`${fmtDuration(doneMs)} done + ${fmtDuration(toGoMs)} to go`}>
      <div className="load-text">
        <span>
          <strong>{fmtDuration(projectedMs)}</strong> planned{goalMs > 0 && <> · goal {fmtDuration(goalMs)}</>}
          {unestimated > 0 && <> · +{unestimated} without an estimate</>}
        </span>
        {right && <span className={overMs > 0 ? "over" : ""}>{right}</span>}
      </div>
      <div className="load-bar" aria-hidden>
        <i className="done" style={{ width: pct(doneMs) }} />
        <i className="togo" style={{ left: pct(doneMs), width: pct(toGoMs) }} />
        {overMs > 0 && <i className="over" style={{ left: pct(goalMs), width: pct(overMs) }} />}
      </div>
    </div>
  );
}

function QuickAdd({ viewing, onAdded }: { viewing: Bucket; onAdded(row: TaskRow): void }) {
  const [text, setText] = useState("");
  const [dest, setDest] = useState<Bucket>(viewing);
  const [parsed, setParsed] = useState<ParsedTask | null>(null);
  const [bad, setBad] = useState(false);
  const seq = useRef(0);

  useEffect(() => setDest(viewing), [viewing]);
  useEffect(() => {
    const n = ++seq.current;
    if (!text.trim()) return setParsed(null);
    api.taskParse(text).then((p) => n === seq.current && setParsed(p));
  }, [text]);

  const shake = () => {
    setBad(true);
    setTimeout(() => setBad(false), 400);
  };
  const add = () => {
    if (!text.trim()) return;
    api.taskAdd(text, dest, false).then(
      (row) => {
        setText("");
        onAdded(row);
      },
      shake,
    );
  };
  const est = parsed && estimateLabel(parsed.estimateMs, 0);

  return (
    <div className="quick-add-wrap">
      <label className={`quick-add${bad ? " bad" : ""}`}>
        <span className="qa-plus" aria-hidden>+</span>
        <span className="sr-only">Add a task</span>
        <input
          value={text}
          maxLength={200}
          placeholder="Add a task   e.g. Prep slides 30m #course"
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              add();
            } else if (e.key === "Tab" && !e.shiftKey && text.trim()) {
              e.preventDefault();
              setDest(DEST_CYCLE[(DEST_CYCLE.indexOf(dest) + 1) % DEST_CYCLE.length]);
            } else if (e.key === "Escape") {
              if (text) setText("");
              else (e.target as HTMLInputElement).blur();
            }
          }}
        />
        <kbd>⏎</kbd>
      </label>
      {text.trim() && (
        <div className="qa-chips" aria-live="polite">
          {est && <span className="qa-chip">{est.text}</span>}
          {parsed?.project && (
            <span className="qa-chip">
              <i style={{ background: projectColor(parsed.project) }} />#{parsed.project}
            </span>
          )}
          <button className="qa-chip dest" title="Tab changes where it goes" onClick={() => setDest(DEST_CYCLE[(DEST_CYCLE.indexOf(dest) + 1) % 3])}>
            → {COLUMN_LABEL[dest]}
          </button>
          <span className="qa-hint">{parsed ? "Tab to change list" : "Needs a title"}</span>
        </div>
      )}
    </div>
  );
}

/** What the clock will do next, and the one main button. */
function Footer({ snap, now, top, focusMin, shortMin }: { snap: Snapshot; now: number; top: TaskRow | null; focusMin: number; shortMin: number }) {
  const focus = snap.phase === "focus";
  const left = fmtClock(remainingOf(snap, now));
  const name = (s: string) => (s.length > 26 ? `${s.slice(0, 25)}…` : s);
  let info: string;
  let button: { label: string; play: boolean; onClick(): void } | null = null;

  if (focus && snap.run !== "idle") {
    info = snap.run === "running" ? `Focus · ${left} left` : `Paused · ${left} left`;
    button = {
      label: snap.run === "running" ? "Pause" : snap.task ? `Continue ${name(snap.task)}` : "Continue",
      play: snap.run === "paused",
      onClick: () => api.act({ type: "toggle" }),
    };
  } else if (!focus) {
    info = `${snap.phase === "long_break" ? "Long break" : "Break"} · ${left}${snap.run === "running" ? " left" : ""}`;
    if (top) button = { label: `Start ${name(top.title)} now`, play: true, onClick: () => api.act({ type: "startTask", value: top.id }) };
  } else {
    info = `Focus ${focusMin}m · then a ${shortMin}m break`;
    button = top
      ? { label: `Start ${name(top.title)}`, play: true, onClick: () => api.act({ type: "startTask", value: top.id }) }
      : { label: "Start focus", play: true, onClick: () => api.act({ type: "toggle" }) };
  }

  return (
    <div className="plan-foot">
      <span>{info}</span>
      {button && (
        <button className="primary big" onClick={button.onClick}>
          {button.play && <PlayIcon />}
          {button.label}
        </button>
      )}
    </div>
  );
}

interface Toast {
  text: string;
  undo?: boolean;
}

function ToastBar({ toast, onClose }: { toast: Toast; onClose(): void }) {
  useEffect(() => {
    const t = setTimeout(onClose, toast.undo ? 6000 : 3000);
    return () => clearTimeout(t);
  }, [toast]);
  return (
    <div className="toast" role="status">
      <span>{toast.text}</span>
      {toast.undo && (
        <button
          className="link"
          onClick={() => {
            onClose();
            api.taskUndoDelete();
          }}
        >
          Undo
        </button>
      )}
    </div>
  );
}
