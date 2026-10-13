import { useEffect, useRef, useState, type CSSProperties } from "react";
import { fmtDuration } from "../format";
import type { Bucket, TaskRow } from "../types";
import { BOARD_BUCKETS, COLUMN_LABEL, estimateLabel, leftoverLabel, projectColor } from "./planModel";

export interface CardActions {
  start(t: TaskRow): void;
  setDone(t: TaskRow, done: boolean): void;
  move(t: TaskRow, bucket: Bucket, beforeId?: number | null): void;
  /** ⌥↑ / ⌥↓ within the column. */
  step(t: TaskRow, delta: -1 | 1): void;
  remove(t: TaskRow): void;
  /** Resolves false when the text has no title. */
  rename(t: TaskRow, text: string): Promise<boolean>;
}

interface Props {
  task: TaskRow;
  live: boolean;
  spentMs: number;
  now: number;
  actions: CardActions;
  dragging: boolean;
  /** Board view: the live card gets its own rows for the timer. */
  board?: boolean;
  /** Board view: the timer's state, shown on the live card. */
  note?: string;
  style?: CSSProperties;
  onPointerDown(e: React.PointerEvent): void;
  /** True when this click ended a drag and should be ignored. */
  consumeClick(): boolean;
}

/** One open task. Shared by the panel and board layouts. */
export function TaskCard({ task: t, live, spentMs, now, actions, dragging, board, note, style, onPointerDown, consumeClick }: Props) {
  const [editing, setEditing] = useState(false);
  const [menu, setMenu] = useState(false);
  const ref = useRef<HTMLLIElement>(null);
  const est = estimateLabel(t.estimateMs, spentMs);
  const pct = live && t.estimateMs ? Math.min(1, spentMs / t.estimateMs) : 0;
  const inline = live && !board;
  const progress = live && t.estimateMs != null && (
    <span className="tc-progress" aria-hidden>
      <i style={{ width: `${pct * 100}%` }} />
    </span>
  );
  const nextBucket: Bucket | null = t.bucket === "week" ? "today" : t.bucket === "backlog" ? "week" : null;

  const onKey = (e: React.KeyboardEvent) => {
    if (e.target !== ref.current || editing) return;
    const side = BOARD_BUCKETS.indexOf(t.bucket);
    let handled = true;
    if (e.altKey && e.key === "ArrowUp") actions.step(t, -1);
    else if (e.altKey && e.key === "ArrowDown") actions.step(t, 1);
    else if (e.altKey && e.key === "ArrowLeft" && side > 0) actions.move(t, BOARD_BUCKETS[side - 1]);
    else if (e.altKey && e.key === "ArrowRight" && side < BOARD_BUCKETS.length - 1) actions.move(t, BOARD_BUCKETS[side + 1]);
    else if (e.key === "ArrowUp" || e.key === "ArrowDown") focusSibling(ref.current, e.key === "ArrowUp" ? -1 : 1);
    else if (e.key === " ") actions.setDone(t, true);
    else if (e.key === "Enter") setEditing(true);
    else if (e.key === "Backspace" || e.key === "Delete") actions.remove(t);
    else handled = false;
    if (handled) e.preventDefault();
  };

  return (
    <li
      ref={ref}
      className={`task-card${live ? " live" : ""}${t.project || t.leftover ? "" : " no-meta"}${dragging ? " dragging" : ""}${menu ? " menu-open" : ""}`}
      data-drag-id={t.id}
      tabIndex={0}
      aria-label={`${t.title}${t.project ? `, #${t.project}` : ""}${est ? `, ${est.text}` : ""}${live ? ", live" : ""}`}
      style={style}
      onPointerDown={onPointerDown}
      onClickCapture={(e) => {
        if (consumeClick()) {
          e.stopPropagation();
          e.preventDefault();
        }
      }}
      onKeyDown={onKey}
    >
      <button className="check" aria-label={`Mark ${t.title} done`} title="Done" onClick={() => actions.setDone(t, true)} />
      <div className="tc-main">
        {editing ? (
          <TitleEditor
            initial={t.title}
            onDone={async (text) => {
              if (text !== null && text !== t.title && !(await actions.rename(t, text))) return false;
              setEditing(false);
              ref.current?.focus();
              return true;
            }}
          />
        ) : (
          <span className="tc-title" title="Click to edit" onClick={() => setEditing(true)}>
            {t.title}
          </span>
        )}
        {(inline || t.project || t.leftover) && (
          <div className="tc-meta">
            {inline && <span className="live-tag">Live</span>}
            {t.project && (
              <span className="tc-project">
                <i style={{ background: projectColor(t.project) }} />
                <span>#{t.project}</span>
              </span>
            )}
            {t.leftover && <span className="tc-from">{leftoverLabel(t.movedAt, now)}</span>}
            {inline && progress}
          </div>
        )}
        {live && board && (
          <div className="tc-live">
            <span className="live-tag">Live</span>
            {note && <span className="tc-note">{note}</span>}
          </div>
        )}
        {live && board && progress}
      </div>
      {est && <span className={`tc-est${est.over ? " over" : ""}${live ? " live" : ""}`}>{est.text}</span>}
      {!editing && (
        <div className="tc-actions">
          {t.bucket === "today" && !live && (
            <button className="tc-btn" onClick={() => actions.start(t)} title="Start focus on this task">
              <PlayIcon /> <span className="label">Start</span>
            </button>
          )}
          {nextBucket && (
            <button className="tc-btn" onClick={() => actions.move(t, nextBucket)} title={`Move to ${COLUMN_LABEL[nextBucket]}`}>
              → <span className="label">{COLUMN_LABEL[nextBucket]}</span>
            </button>
          )}
          <button className="tc-btn icon" aria-label="More" aria-haspopup="menu" aria-expanded={menu} onClick={() => setMenu(!menu)}>
            ⋯
          </button>
          {menu && (
            <CardMenu
              task={t}
              onClose={() => setMenu(false)}
              onEdit={() => setEditing(true)}
              onMove={(b) => actions.move(t, b)}
              onDelete={() => actions.remove(t)}
            />
          )}
        </div>
      )}
    </li>
  );
}

function TitleEditor({ initial, onDone }: { initial: string; onDone(text: string | null): Promise<boolean> }) {
  const [text, setText] = useState(initial);
  const [bad, setBad] = useState(false);
  const busy = useRef(false);
  const save = async (value: string | null) => {
    if (busy.current) return;
    busy.current = true;
    const ok = await onDone(value === null ? null : value.trim());
    busy.current = false;
    if (!ok) {
      setBad(true);
      setTimeout(() => setBad(false), 400);
    }
  };
  return (
    <input
      className={`tc-edit${bad ? " bad" : ""}`}
      value={text}
      autoFocus
      maxLength={200}
      aria-label="Task title. Add a duration like 45m to change the estimate."
      onChange={(e) => setText(e.target.value)}
      onFocus={(e) => e.target.select()}
      onBlur={() => save(text)}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") save(text);
        else if (e.key === "Escape") save(null);
      }}
    />
  );
}

function CardMenu({ task, onClose, onEdit, onMove, onDelete }: { task: TaskRow; onClose(): void; onEdit(): void; onMove(b: Bucket): void; onDelete(): void }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const away = (e: PointerEvent) => !ref.current?.parentElement?.contains(e.target as Node) && onClose();
    addEventListener("pointerdown", away, true);
    ref.current?.querySelector("button")?.focus();
    return () => removeEventListener("pointerdown", away, true);
  }, []);
  const onKey = (e: React.KeyboardEvent) => {
    const items = [...(ref.current?.querySelectorAll("button") ?? [])];
    const i = items.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === "Escape") {
      onClose();
      (ref.current?.previousElementSibling as HTMLElement | null)?.focus();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      items[(i + (e.key === "ArrowDown" ? 1 : items.length - 1)) % items.length]?.focus();
    } else return;
    e.preventDefault();
  };
  const pick = (fn: () => void) => () => {
    onClose();
    fn();
  };
  return (
    <div className="tc-menu" role="menu" ref={ref} data-nodrag onKeyDown={onKey}>
      <button role="menuitem" onClick={pick(onEdit)}>Edit</button>
      {(["today", "week", "backlog"] as Bucket[])
        .filter((b) => b !== task.bucket)
        .map((b) => (
          <button key={b} role="menuitem" onClick={pick(() => onMove(b))}>
            Move to {COLUMN_LABEL[b]}
          </button>
        ))}
      <hr />
      <button role="menuitem" className="danger" onClick={pick(onDelete)}>Delete</button>
    </div>
  );
}

/** A finished task: tick to reopen it. */
export function DoneCard({ task: t, onReopen }: { task: TaskRow; onReopen(): void }) {
  return (
    <li className="task-card done">
      <button className="check on" aria-label={`Reopen ${t.title}`} title="Reopen" onClick={onReopen}>
        <CheckIcon />
      </button>
      <div className="tc-main">
        <span className="tc-title">{t.title}</span>
        {t.project && (
          <div className="tc-meta">
            <span className="tc-project">
              <i style={{ background: projectColor(t.project) }} />#{t.project}
            </span>
          </div>
        )}
      </div>
      <span className="tc-est">{t.spentMs > 0 ? fmtDuration(t.spentMs) : "no time tracked"}</span>
    </li>
  );
}

function focusSibling(el: HTMLElement | null, dir: -1 | 1) {
  const cards = [...document.querySelectorAll<HTMLElement>(".task-card[tabindex]")];
  const i = el ? cards.indexOf(el) : -1;
  cards[i + dir]?.focus();
}

export const PlayIcon = () => (
  <svg width="9" height="10" viewBox="0 0 9 10" aria-hidden>
    <path d="M1 1.2v7.6a.6.6 0 0 0 .9.5l6.3-3.8a.6.6 0 0 0 0-1L1.9.7A.6.6 0 0 0 1 1.2Z" fill="currentColor" />
  </svg>
);

const CheckIcon = () => (
  <svg width="11" height="11" viewBox="0 0 12 12" aria-hidden>
    <path d="M2.5 6.3 5 8.7l4.5-5.2" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
  </svg>
);
