// Pointer drag and drop for task lists, written by hand: HTML5 drag and drop
// fights Tauri's file-drop handling. Lists are marked with
// data-drop-list="<column>", cards with data-drag-id, and other drop targets
// (the column tabs) with data-drop-column="<column>".
//
// Layout is measured once when a drag starts. While dragging, cards only move
// with transforms, so those measurements stay valid and nothing jitters.

import { useEffect, useRef, useState, type CSSProperties } from "react";
import type { Column } from "./planModel";

export interface Drop {
  column: Column;
  /** null = at the end. */
  beforeId: number | null;
}

/** Where among the other cards (by their vertical midpoints) a drop at `y` lands. */
export function dropIndex(mids: number[], y: number): number {
  let i = 0;
  while (i < mids.length && y > mids[i]) i++;
  return i;
}

/** The card a drop at `index` goes before, in the list without the dragged card. */
export function beforeIdAt(others: number[], index: number): number | null {
  return others[index] ?? null;
}

/**
 * Keyboard moves: the `beforeId` that moves `id` by `delta` places within
 * `ids`, or undefined when it is already at that end.
 */
export function stepBefore(ids: number[], id: number, delta: -1 | 1): number | null | undefined {
  const s = ids.indexOf(id);
  const t = s + delta;
  if (s < 0 || t < 0 || t >= ids.length) return undefined;
  return delta < 0 ? ids[t] : (ids[t + 1] ?? null);
}

interface Measured {
  column: Column;
  left: number;
  right: number;
  items: { id: number; top: number; height: number }[];
}

interface DragState {
  id: number;
  from: Column;
  dx: number;
  dy: number;
  /** Height of the gap the card leaves, including the space below it. */
  size: number;
  over: { column: Column; index: number; list: boolean } | null;
}

const THRESHOLD = 4;
const EDGE = 56;
/** How far outside a list a drop still counts, to cover the gap between columns. */
const GAP = 24;

export function useDragList(onDrop: (id: number, drop: Drop) => void) {
  const [drag, setDrag] = useState<DragState | null>(null);
  const [settling, setSettling] = useState(false);
  const moved = useRef(false);
  const dropRef = useRef(onDrop);
  dropRef.current = onDrop;
  const cleanup = useRef<(() => void) | null>(null);

  useEffect(() => () => cleanup.current?.(), []);

  useEffect(() => {
    if (!settling) return;
    const r = requestAnimationFrame(() => setSettling(false));
    return () => cancelAnimationFrame(r);
  }, [settling]);

  const start = (e: React.PointerEvent, id: number, from: Column) => {
    if (e.button !== 0 || e.pointerType === "touch") return;
    if ((e.target as HTMLElement).closest("button, input, textarea, a, [data-nodrag]")) return;
    const x0 = e.clientX;
    const y0 = e.clientY + scrollY;
    let lists: Measured[] = [];
    let state: DragState | null = null;
    moved.current = false;

    const measure = () => {
      lists = [...document.querySelectorAll<HTMLElement>("[data-drop-list]")].map((el) => {
        const r = el.getBoundingClientRect();
        return {
          column: el.dataset.dropList as Column,
          left: r.left,
          right: r.right,
          items: [...el.querySelectorAll<HTMLElement>("[data-drag-id]")].map((c) => {
            const cr = c.getBoundingClientRect();
            return { id: Number(c.dataset.dragId), top: cr.top + scrollY, height: cr.height };
          }),
        };
      });
      const src = lists.find((l) => l.column === from);
      const i = src?.items.findIndex((it) => it.id === id) ?? -1;
      const me = src?.items[i];
      const next = src?.items[i + 1];
      return me ? (next ? next.top - me.top : me.height) : 0;
    };

    const locate = (cx: number, cy: number): DragState["over"] => {
      const tab = document.elementFromPoint(cx, cy)?.closest<HTMLElement>("[data-drop-column]");
      if (tab) return { column: tab.dataset.dropColumn as Column, index: -1, list: false };
      // The list under the pointer, or the nearest one when it is in the gap between columns.
      const gap = (l: Measured) => Math.max(l.left - cx, cx - l.right, 0);
      const list = lists.reduce<Measured | null>((best, l) => (gap(l) <= GAP && (!best || gap(l) < gap(best)) ? l : best), null);
      if (!list) return null;
      const others = list.items.filter((it) => it.id !== id);
      return { column: list.column, index: dropIndex(others.map((it) => it.top + it.height / 2), cy + scrollY), list: true };
    };

    const onMove = (ev: PointerEvent) => {
      const dx = ev.clientX - x0;
      const dy = ev.clientY + scrollY - y0;
      if (!state) {
        if (Math.hypot(dx, dy) < THRESHOLD) return;
        moved.current = true;
        state = { id, from, dx: 0, dy: 0, size: measure(), over: null };
        document.documentElement.classList.add("dragging-task");
      }
      if (ev.clientY < EDGE) scrollBy(0, -8);
      else if (ev.clientY > innerHeight - EDGE) scrollBy(0, 8);
      state = { ...state, dx, dy: ev.clientY + scrollY - y0, over: locate(ev.clientX, ev.clientY) };
      setDrag(state);
    };

    const finish = (commit: boolean) => {
      cleanup.current?.();
      if (!state) return;
      const s = state;
      state = null;
      document.documentElement.classList.remove("dragging-task");
      // The click that ends a drag fires right after pointerup; let it see `moved`.
      setTimeout(() => (moved.current = false), 0);
      setSettling(true);
      setDrag(null);
      if (!commit || !s.over) return;
      const list = lists.find((l) => l.column === s.over!.column);
      const others = (list?.items ?? []).map((it) => it.id).filter((x) => x !== id);
      if (s.over.list) {
        const ids = list!.items.map((it) => it.id);
        if (s.over.column === s.from && ids.indexOf(id) === s.over.index) return;
        dropRef.current(id, { column: s.over.column, beforeId: beforeIdAt(others, s.over.index) });
      } else if (s.over.column !== s.from) {
        dropRef.current(id, { column: s.over.column, beforeId: null });
      }
    };
    const onUp = () => finish(true);
    const onKey = (ev: KeyboardEvent) => ev.key === "Escape" && finish(false);
    const onCancel = () => finish(false);

    cleanup.current?.();
    addEventListener("pointermove", onMove);
    addEventListener("pointerup", onUp);
    addEventListener("pointercancel", onCancel);
    addEventListener("keydown", onKey, true);
    cleanup.current = () => {
      removeEventListener("pointermove", onMove);
      removeEventListener("pointerup", onUp);
      removeEventListener("pointercancel", onCancel);
      removeEventListener("keydown", onKey, true);
      cleanup.current = null;
    };
  };

  /** Transform for a card while something is being dragged. */
  const styleFor = (id: number, column: Column, ids: number[]): CSSProperties | undefined => {
    if (!drag) return undefined;
    if (id === drag.id) return { transform: `translate(${drag.dx}px, ${drag.dy}px)` };
    const over = drag.over?.list ? drag.over : null;
    const index = ids.indexOf(id);
    let shift = 0;
    if (column === drag.from) {
      const sourceIndex = ids.indexOf(drag.id);
      // Close the gap the card left…
      const o = index > sourceIndex ? index - 1 : index;
      if (index > sourceIndex) shift -= drag.size;
      // …and open one where it would land.
      if (over?.column === column && o >= over.index) shift += drag.size;
    } else if (over?.column === column && index >= over.index) {
      shift += drag.size;
    }
    return shift ? { transform: `translateY(${shift}px)` } : undefined;
  };

  return {
    drag,
    settling,
    start,
    styleFor,
    /** True right after a drag, so the click that ends it can be ignored. */
    consumeClick: () => {
      const m = moved.current;
      moved.current = false;
      return m;
    },
  };
}
