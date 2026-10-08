// Building blocks shared by every widget layout. Layouts only decide where
// these go; behaviour lives here once.

import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { fmtClock, MINUTE, parseDuration } from "../format";
import { IconHide, IconMenu, IconPause, IconPlay, IconReset, IconSkip } from "../icons";
import type { Action, Settings, Snapshot } from "../types";
import { Dial } from "./Dial";
import { Ring } from "./Ring";
import { SevenSeg } from "./SevenSeg";

export type Editing = "time" | "task" | null;

export interface Model {
  snap: Snapshot;
  settings: Settings;
  remaining: number;
  /** Remaining / planned, 1 → 0. */
  fraction: number;
  editing: Editing;
  setEditing: (e: Editing) => void;
  hideKey: string;
}

export const act = (a: Action) => void api.act(a);

export const PLATE: Record<Snapshot["phase"], string> = {
  focus: "FOCUS",
  short_break: "SHORT BREAK",
  long_break: "LONG BREAK",
};

export const casingClass = (m: Model) => `casing phase-${m.snap.phase} run-${m.snap.run}`;

export function subLabel(snap: Snapshot): string {
  return snap.phase === "focus" ? `SESSION ${snap.sessionNumber}` : `${snap.completedToday} DONE`;
}

/** Scroll to nudge ±1 minute; trackpads send many small deltas, so accumulate. */
export function useWheelAdjust() {
  const acc = useRef(0);
  return (e: React.WheelEvent) => {
    acc.current += e.deltaY;
    const steps = Math.trunc(acc.current / 40);
    if (steps !== 0) {
      acc.current -= steps * 40;
      act({ type: "adjust", value: -steps * MINUTE });
    }
  };
}

export function Digits({ m }: { m: Model }) {
  if (m.editing === "time") return <TimeInput initial={m.remaining} onDone={() => m.setEditing(null)} />;
  const text = fmtClock(m.remaining);
  return (
    <button className="digits" title="Click to set time · scroll to nudge ±1 min" onClick={() => m.setEditing("time")}>
      {m.settings.style === "retro" ? (
        <SevenSeg text={text} blinkColon={m.snap.run === "paused"} />
      ) : (
        <span className={m.snap.run === "paused" ? "mdigits paused" : "mdigits"}>{text}</span>
      )}
    </button>
  );
}

export function Pips({ m }: { m: Model }) {
  const { snap } = m;
  return (
    <span className="pips" aria-label={`${snap.cycle} of ${snap.longEvery} until long break`}>
      {Array.from({ length: snap.longEvery }, (_, i) => (
        <i key={i} className={i < snap.cycle ? "full" : i === snap.cycle && snap.phase === "focus" && snap.run !== "idle" ? "live" : ""} />
      ))}
    </span>
  );
}

export function Nudges() {
  return (
    <>
      <button className="nudge" onClick={() => act({ type: "adjust", value: -5 * MINUTE })} title="Remove 5 minutes">−5</button>
      <button className="nudge" onClick={() => act({ type: "adjust", value: 5 * MINUTE })} title="Add 5 minutes">+5</button>
      <button className="nudge" onClick={() => act({ type: "adjust", value: 10 * MINUTE })} title="Add 10 minutes">+10</button>
    </>
  );
}

export function MainKey({ m, round = false }: { m: Model; round?: boolean }) {
  const running = m.snap.run === "running";
  return (
    <button className={round ? "key main round" : "key main"} title={running ? "Pause (Space)" : "Start (Space)"} onClick={() => act({ type: "toggle" })}>
      {running ? <IconPause /> : <IconPlay />}
    </button>
  );
}

export function Controls({ m, mini = false }: { m: Model; mini?: boolean }) {
  const isFocus = m.snap.phase === "focus";
  const side = mini ? "key mini" : "key small";
  return (
    <div className={mini ? "controls mini" : "controls"}>
      <button className={side} title="Reset (R) · ⇧R restarts" onClick={(e) => act({ type: e.shiftKey ? "restart" : "reset" })}>
        <IconReset size={mini ? 11 : 14} />
      </button>
      {mini ? (
        <button className="key mini primary" title={m.snap.run === "running" ? "Pause (Space)" : "Start (Space)"} onClick={() => act({ type: "toggle" })}>
          {m.snap.run === "running" ? <IconPause size={12} /> : <IconPlay size={12} />}
        </button>
      ) : (
        <MainKey m={m} />
      )}
      <button className={side} title={isFocus ? "Skip to break (S)" : "Skip to focus (S)"} onClick={() => act({ type: "skip" })}>
        <IconSkip size={mini ? 11 : 14} />
      </button>
    </div>
  );
}

export function CapButtons({ m, soft = false }: { m: Model; soft?: boolean }) {
  const cls = soft ? "cap-btn soft" : "cap-btn";
  return (
    <>
      <button className={cls} title={`Hide clock (${m.hideKey}). The timer keeps running.`} onClick={() => api.toggleClock()}>
        <IconHide />
      </button>
      <button className={cls} title="Menu: widget size, style, modes…" onClick={() => api.showClockMenu()}>
        <IconMenu />
      </button>
    </>
  );
}

/**
 * The round dial with its readout, drawn at 166px and scaled to `size`.
 * `hoverRow` is what replaces the session line on hover.
 */
export function DialUnit({
  m,
  size,
  plate = true,
  sub = true,
  hoverRow,
}: {
  m: Model;
  size: number;
  plate?: boolean;
  sub?: boolean;
  hoverRow: "nudges" | "controls";
}) {
  const onWheel = useWheelAdjust();
  return (
    <div className="dial-slot" style={{ width: size, height: size }}>
      <div className="well" style={{ transform: `scale(${size / 166})` }} onWheel={onWheel}>
        {m.settings.style === "retro" ? <Dial fraction={m.fraction} /> : <Ring fraction={m.fraction} />}
        <div className="readout">
          {plate && <div className="plate">{PLATE[m.snap.phase]}</div>}
          <Digits m={m} />
          {sub && (
            <div className="sub">
              <Pips m={m} />
              <span className="sub-label">{subLabel(m.snap)}</span>
            </div>
          )}
          <div className={`hover-row ${hoverRow}`}>{hoverRow === "nudges" ? <Nudges /> : <Controls m={m} mini />}</div>
        </div>
      </div>
    </div>
  );
}

export function TaskLine({ m }: { m: Model }) {
  const { snap } = m;
  const editing = m.editing === "task";
  const [draft, setDraft] = useState("");
  useEffect(() => {
    if (editing) setDraft([snap.task, snap.project && `#${snap.project}`].filter(Boolean).join(" "));
  }, [editing]);
  const done = (save: boolean) => {
    if (save) act({ type: "setTask", value: draft.trim() || null });
    m.setEditing(null);
  };

  if (editing) {
    return (
      <div className="task">
        <input
          autoFocus
          className="task-input"
          maxLength={120}
          placeholder="What are you working on?  #project"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") done(true);
            if (e.key === "Escape") done(false);
            e.stopPropagation();
          }}
          onBlur={() => done(true)}
        />
      </div>
    );
  }

  if (snap.awayMs !== null && snap.run === "paused") {
    return (
      <div className="task">
        <button className="task-text away" title="Chronos paused the session while you were away. Press play to continue." onClick={() => act({ type: "toggle" })}>
          Paused · away {Math.max(1, Math.round(snap.awayMs / 60_000))}m. Resume?
        </button>
      </div>
    );
  }

  let text: string;
  let cls = "task-text";
  if (snap.phase === "short_break") text = snap.run === "idle" ? "Nice work. Take five?" : "Breathe. Back soon.";
  else if (snap.phase === "long_break") text = snap.run === "idle" ? "Set done. Take a proper break." : "Step away for a bit.";
  else if (snap.task) text = snap.task;
  else if (snap.project) text = "";
  else {
    text = "+ what are you working on?";
    cls += " placeholder";
  }

  return (
    <div className="task">
      <button
        className={cls}
        title={snap.task ? `Focusing on: ${snap.task} (click to change)` : "Set an intention (T). Add #project to tag it."}
        onClick={() => m.setEditing("task")}
      >
        {snap.phase === "focus" && snap.task && <span className="task-mark">▸</span>}
        {text}
        {snap.phase === "focus" && snap.project && <span className="task-project">#{snap.project}</span>}
      </button>
    </div>
  );
}

function TimeInput({ initial, onDone }: { initial: number; onDone: () => void }) {
  const [value, setValue] = useState(String(Math.ceil(initial / MINUTE)));
  const [bad, setBad] = useState(false);
  const commit = () => {
    const ms = parseDuration(value);
    if (ms === null) {
      setBad(true);
      return;
    }
    act({ type: "setRemaining", value: ms });
    onDone();
  };
  return (
    <form
      className={bad ? "time-edit bad" : "time-edit"}
      onSubmit={(e) => {
        e.preventDefault();
        commit();
      }}
    >
      <input
        autoFocus
        onFocus={(e) => e.target.select()}
        value={value}
        onChange={(e) => {
          setValue(e.target.value);
          setBad(false);
        }}
        onKeyDown={(e) => {
          if (e.key === "Escape") onDone();
          e.stopPropagation();
        }}
        onBlur={onDone}
        aria-label="Minutes remaining"
      />
      <span>min</span>
    </form>
  );
}
