// The morning sheet: once a day, over the board, "pick what goes in Today".
// Everything already in Today starts ticked; This week and the backlog are
// offered unticked. Start the day makes Today exactly the ticked tasks.

import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { fmtDuration } from "../format";
import { useSnapshot } from "../hooks";
import type { MorningState, TaskRow } from "../types";
import { LoadMeter } from "./PlanColumns";
import { estimateLabel, leftoverLabel, projectColor, projectedLoad } from "./planModel";
import { useShake } from "./useTaskDraft";

interface Props {
  state: MorningState;
  onStart(): void;
  onNotToday(): void;
}

export function Morning({ state, onStart, onNotToday }: Props) {
  const snap = useSnapshot();
  const [added, setAdded] = useState<TaskRow[]>([]);
  const [ticked, setTicked] = useState(() => new Set(state.today.map((t) => t.id)));
  const [showBacklog, setShowBacklog] = useState(false);
  const [busy, setBusy] = useState(false);
  const sheet = useRef<HTMLDivElement>(null);

  const leftover = state.today.filter((t) => t.leftover);
  const already = state.today.filter((t) => !t.leftover);
  // The order shown is the order Today gets.
  const all = [...leftover, ...already, ...state.week, ...(showBacklog ? state.backlog : []), ...added];
  const chosen = all.filter((t) => ticked.has(t.id));
  const load = projectedLoad(chosen, snap?.focusMsToday ?? 0, snap?.goalMs ?? 0, (t) => t.spentMs);

  const notToday = () => {
    api.morningDismiss();
    onNotToday();
  };
  useEffect(() => sheet.current?.querySelector<HTMLElement>("button.primary")?.focus(), []);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && !busy && notToday();
    addEventListener("keydown", onKey);
    return () => removeEventListener("keydown", onKey);
  }, [busy]);

  const toggle = (id: number) =>
    setTicked((s) => {
      const next = new Set(s);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  const start = async () => {
    setBusy(true);
    try {
      await api.morningCommit(chosen.map((t) => t.id));
      onStart();
    } finally {
      setBusy(false);
    }
  };

  const group = (label: string, rows: TaskRow[]) =>
    rows.length > 0 && (
      <div className="m-group">
        <span className="m-label">{label}</span>
        {rows.map((t) => (
          <Choice key={t.id} task={t} on={ticked.has(t.id)} onToggle={() => toggle(t.id)} now={snap?.now ?? Date.now()} />
        ))}
      </div>
    );

  const over = load.goalMs > 0 ? load.projectedMs - load.goalMs : 0;
  return (
    <div className="m-backdrop">
      <div className="m-sheet" ref={sheet} role="dialog" aria-modal="true" aria-labelledby="m-title">
        <div>
          <h3 id="m-title">Good morning</h3>
          <span className="m-sub">Pick what goes in Today.</span>
        </div>
        <div className="m-scroll">
          {group("Left over", leftover)}
          {group("Already in Today", already)}
          {group("From this week", state.week)}
          {showBacklog && group("From the backlog", state.backlog)}
          {state.backlog.length > 0 && !showBacklog && (
            <button className="link m-more" onClick={() => setShowBacklog(true)}>
              Show {state.backlog.length} from the backlog
            </button>
          )}
          {group("Added just now", added)}
          {all.length === 0 && <p className="m-empty">Your lists are empty. Add what you want to get done today.</p>}
          <AddRow
            onAdded={(row) => {
              setAdded((a) => [...a, row]);
              setTicked((s) => new Set(s).add(row.id));
            }}
          />
        </div>
        <div className="m-total">
          <span>
            <strong>{fmtDuration(load.projectedMs)}</strong>
            {load.goalMs > 0 && <> of {fmtDuration(load.goalMs)}</>}
            {load.unestimated > 0 && <> · +{load.unestimated} without an estimate</>}
          </span>
          {over > 0 && <span className="over">{fmtDuration(over)} over</span>}
        </div>
        <LoadMeter load={load} />
        <div className="m-actions">
          <span className="m-count">
            {chosen.length} {chosen.length === 1 ? "task" : "tasks"}
          </span>
          <button className="layout-btn" disabled={busy} onClick={notToday}>
            Not today
          </button>
          <button className="primary big" disabled={busy} onClick={start}>
            Start the day
          </button>
        </div>
      </div>
    </div>
  );
}

function Choice({ task: t, on, onToggle, now }: { task: TaskRow; on: boolean; onToggle(): void; now: number }) {
  const est = estimateLabel(t.estimateMs, t.spentMs);
  return (
    <label className={`m-choice${on ? " on" : ""}`}>
      <input type="checkbox" checked={on} onChange={onToggle} className="sr-only" />
      <span className="m-box" aria-hidden>
        {on && (
          <svg width="10" height="10" viewBox="0 0 12 12">
            <path d="M2.5 6.3 5 8.7l4.5-5.2" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        )}
      </span>
      <span className="m-title">
        {t.title}
        {(t.project || t.leftover) && (
          <small>
            {t.project && (
              <>
                <i style={{ background: projectColor(t.project) }} />#{t.project}
              </>
            )}
            {t.project && t.leftover && " · "}
            {t.leftover && leftoverLabel(t.movedAt, now)}
          </small>
        )}
      </span>
      <span className="m-est">{est?.text ?? "no estimate"}</span>
    </label>
  );
}

function AddRow({ onAdded }: { onAdded(row: TaskRow): void }) {
  const [text, setText] = useState("");
  const [bad, shake] = useShake();
  return (
    <label className={`quick-add m-add${bad ? " bad" : ""}`}>
      <span className="qa-plus" aria-hidden>+</span>
      <span className="sr-only">Add another task for today</span>
      <input
        value={text}
        maxLength={200}
        placeholder="Add another task for today   e.g. Call mom 15m"
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key !== "Enter") return;
          e.preventDefault();
          if (!text.trim()) return;
          api.taskAdd(text, "today", false).then((row) => {
            setText("");
            onAdded(row);
          }, shake);
        }}
      />
    </label>
  );
}
