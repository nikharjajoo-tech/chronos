// Plan tab helpers with no React in them, so they can be unit-tested.

import { MINUTE } from "../format";
import type { Bucket, Snapshot, TaskRow } from "../types";

/** A Plan column: the three buckets plus Done. */
export type Column = Bucket | "done";

export const COLUMN_LABEL: Record<Column, string> = {
  today: "Today",
  week: "This week",
  backlog: "Backlog",
  done: "Done",
};

/** Left to right on the board; ⌥← and ⌥→ step along it. */
export const BOARD_BUCKETS: Bucket[] = ["backlog", "week", "today"];

/** The panel layout's column switcher. */
export const PANEL_COLUMNS: Column[] = ["today", "week", "backlog", "done"];

const DAY = 86_400_000;

export function startOfDay(t: number): number {
  const d = new Date(t);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

/** Open tasks in a bucket, top first. */
export function openIn(tasks: TaskRow[], bucket: Bucket): TaskRow[] {
  return tasks.filter((t) => t.bucket === bucket && !t.done).sort((a, b) => a.position - b.position || a.id - b.id);
}

/** Tasks ticked off since `since`, newest first. */
export function doneSince(tasks: TaskRow[], since: number, until = Infinity): TaskRow[] {
  return tasks
    .filter((t) => t.done && t.doneAt != null && t.doneAt >= since && t.doneAt < until)
    .sort((a, b) => (b.doneAt ?? 0) - (a.doneAt ?? 0));
}

/** Focus time on a task, counting the running block for the live one. */
export function spentOf(t: TaskRow, snap: Snapshot | null, now: number): number {
  if (!snap || snap.taskId !== t.id) return t.spentMs;
  const running = snap.phase === "focus" && snap.run === "running";
  return snap.taskSpentMs + (running ? Math.max(0, now - snap.now) : 0);
}

export interface Load {
  doneMs: number;
  toGoMs: number;
  /** Focus done today plus what the open Today tasks still need. */
  projectedMs: number;
  goalMs: number;
  /** Open Today tasks with no estimate; they are not counted as zero. */
  unestimated: number;
}

/** projected = focus done today + Σ max(estimate − spent, 0) over open Today tasks. */
export function projectedLoad(today: TaskRow[], focusDoneMs: number, goalMs: number, spent: (t: TaskRow) => number): Load {
  let toGoMs = 0;
  let unestimated = 0;
  for (const t of today) {
    if (t.done) continue;
    if (t.estimateMs == null) unestimated++;
    else toGoMs += Math.max(0, t.estimateMs - spent(t));
  }
  return { doneMs: focusDoneMs, toGoMs, projectedMs: focusDoneMs + toGoMs, goalMs, unestimated };
}

/**
 * The list after moving `id` into `bucket` just before `beforeId` (null = at
 * the end), so a drop shows at once instead of waiting for the refetch. The
 * engine works out the real position.
 */
export function applyMove(tasks: TaskRow[], id: number, bucket: Bucket, beforeId: number | null): TaskRow[] {
  const col = openIn(tasks, bucket).filter((t) => t.id !== id);
  const i = beforeId == null ? -1 : col.findIndex((t) => t.id === beforeId);
  const position =
    col.length === 0 ? 1
    : i < 0 ? col[col.length - 1].position + 1
    : i === 0 ? col[0].position - 1
    : (col[i - 1].position + col[i].position) / 2;
  return tasks.map((t) => (t.id === id ? { ...t, bucket, position, leftover: t.leftover && bucket === "today" } : t));
}

// System colours that sit well on light and dark glass. Orange and red are
// left out: they mean "live" and "over".
const PROJECT_COLORS = ["#007aff", "#af52de", "#34c759", "#30b0c7", "#5856d6", "#ff2d55", "#a2845e", "#00c7be"];

/** A stable colour per project, so #job-search looks the same everywhere. */
export function projectColor(name: string): string {
  let h = 2166136261;
  for (let i = 0; i < name.length; i++) {
    h ^= name.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return PROJECT_COLORS[(h >>> 0) % PROJECT_COLORS.length];
}

const mins = (ms: number) => Math.floor(ms / MINUTE);

/** "45m", "1h 30m", or once time is spent "38/60m". */
export function estimateLabel(estimateMs: number | null, spentMs: number): { text: string; over: boolean } | null {
  const spent = mins(spentMs);
  if (estimateMs == null) return spent > 0 ? { text: `${spent}m`, over: false } : null;
  const est = Math.round(estimateMs / MINUTE);
  if (spent > 0) return { text: `${spent}/${est}m`, over: spent > est };
  const h = Math.floor(est / 60);
  const m = est % 60;
  return { text: h === 0 ? `${m}m` : m === 0 ? `${h}h` : `${h}h ${m}m`, over: false };
}

/** "from yesterday", "from Fri", "from 3 Oct". */
export function leftoverLabel(movedAt: number, now: number): string {
  const days = Math.round((startOfDay(now) - startOfDay(movedAt)) / DAY);
  if (days <= 1) return "from yesterday";
  const d = new Date(movedAt);
  if (days < 7) return `from ${d.toLocaleDateString([], { weekday: "short" })}`;
  return `from ${d.toLocaleDateString([], { day: "numeric", month: "short" })}`;
}
