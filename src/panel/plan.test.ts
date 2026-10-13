import { describe, expect, it } from "vitest";
import { MINUTE } from "../format";
import type { TaskRow } from "../types";
import { applyMove, estimateLabel, openIn, projectColor, projectedLoad } from "./planModel";
import { beforeIdAt, dropIndex, stepBefore } from "./useDragList";

const task = (id: number, extra: Partial<TaskRow> = {}): TaskRow => ({
  id, title: `Task ${id}`, project: null, bucket: "today", position: id, estimateMs: null, done: false,
  createdAt: 0, movedAt: 0, doneAt: null, spentMs: 0, leftover: false, note: null, source: null, ...extra,
});

describe("drop maths", () => {
  // Three other cards, 40px tall, midpoints at 20, 60 and 100.
  const mids = [20, 60, 100];
  const others = [7, 8, 9];

  it("drops at the top", () => {
    expect(dropIndex(mids, 5)).toBe(0);
    expect(beforeIdAt(others, dropIndex(mids, 5))).toBe(7);
  });
  it("drops in the middle", () => {
    expect(dropIndex(mids, 61)).toBe(2);
    expect(beforeIdAt(others, 2)).toBe(9);
  });
  it("drops at the end", () => {
    expect(dropIndex(mids, 400)).toBe(3);
    expect(beforeIdAt(others, 3)).toBeNull();
  });
  it("drops into an empty list", () => {
    expect(dropIndex([], 10)).toBe(0);
    expect(beforeIdAt([], 0)).toBeNull();
  });
});

describe("keyboard moves", () => {
  const ids = [1, 2, 3, 4];
  it("moves up before the card above", () => expect(stepBefore(ids, 3, -1)).toBe(2));
  it("moves down before the card two below", () => expect(stepBefore(ids, 2, 1)).toBe(4));
  it("moves down to the end", () => expect(stepBefore(ids, 3, 1)).toBeNull());
  it("stays put at either end", () => {
    expect(stepBefore(ids, 1, -1)).toBeUndefined();
    expect(stepBefore(ids, 4, 1)).toBeUndefined();
  });
});

describe("applyMove", () => {
  const tasks = [task(1), task(2), task(3), task(4, { bucket: "week", position: 1 })];
  const order = (ts: TaskRow[], b: TaskRow["bucket"]) => openIn(ts, b).map((t) => t.id);

  it("reorders within a bucket", () => {
    expect(order(applyMove(tasks, 3, "today", 1), "today")).toEqual([3, 1, 2]);
    expect(order(applyMove(tasks, 1, "today", 3), "today")).toEqual([2, 1, 3]);
    expect(order(applyMove(tasks, 1, "today", null), "today")).toEqual([2, 3, 1]);
  });
  it("moves across buckets", () => {
    const next = applyMove(tasks, 4, "today", 2);
    expect(order(next, "today")).toEqual([1, 4, 2, 3]);
    expect(order(next, "week")).toEqual([]);
  });
  it("into an empty bucket", () => expect(order(applyMove(tasks, 2, "backlog", null), "backlog")).toEqual([2]));
});

describe("projectedLoad", () => {
  const spent = (t: TaskRow) => t.spentMs;

  it("adds what is left of each estimate to focus done", () => {
    const today = [
      task(1, { estimateMs: 60 * MINUTE, spentMs: 38 * MINUTE }),
      task(2, { estimateMs: 45 * MINUTE }),
    ];
    const load = projectedLoad(today, 70 * MINUTE, 180 * MINUTE, spent);
    expect(load.toGoMs).toBe((22 + 45) * MINUTE);
    expect(load.projectedMs).toBe((70 + 67) * MINUTE);
    expect(load.unestimated).toBe(0);
  });
  it("counts an overrun task as nothing left, not negative", () => {
    const load = projectedLoad([task(1, { estimateMs: 30 * MINUTE, spentMs: 50 * MINUTE })], 50 * MINUTE, 0, spent);
    expect(load.toGoMs).toBe(0);
    expect(load.projectedMs).toBe(50 * MINUTE);
  });
  it("lists tasks without an estimate instead of counting them as zero", () => {
    const load = projectedLoad([task(1), task(2, { estimateMs: 20 * MINUTE }), task(3)], 0, 0, spent);
    expect(load.unestimated).toBe(2);
    expect(load.toGoMs).toBe(20 * MINUTE);
  });
  it("ignores finished tasks", () => {
    const load = projectedLoad([task(1, { estimateMs: 20 * MINUTE, done: true })], 0, 0, spent);
    expect(load.toGoMs).toBe(0);
  });
});

describe("labels", () => {
  it("shows the estimate, then spent/estimate once time is tracked", () => {
    expect(estimateLabel(45 * MINUTE, 0)).toEqual({ text: "45m", over: false });
    expect(estimateLabel(90 * MINUTE, 0)).toEqual({ text: "1h 30m", over: false });
    expect(estimateLabel(60 * MINUTE, 38 * MINUTE + 59_000)).toEqual({ text: "38/60m", over: false });
    expect(estimateLabel(30 * MINUTE, 41 * MINUTE)).toEqual({ text: "41/30m", over: true });
    expect(estimateLabel(null, 12 * MINUTE)).toEqual({ text: "12m", over: false });
    expect(estimateLabel(null, 0)).toBeNull();
  });
  it("gives each project one stable colour", () => {
    expect(projectColor("job-search")).toBe(projectColor("job-search"));
    expect(projectColor("job-search")).toMatch(/^#[0-9a-f]{6}$/);
  });
});
