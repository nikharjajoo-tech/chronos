import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, isTauri } from "../api";
import { MINUTE, prettyShortcut } from "../format";
import { clockStyleClass, remainingOf, useSecondTick, useSettings, useSnapshot } from "../hooks";
import type { Snapshot } from "../types";
import { LAYOUTS, Layout } from "./layouts";
import { act, taskSpent, type Editing, type Model } from "./parts";
import "./clock.css";
import "./widgets.css";

export function Clock() {
  const snap = useSnapshot();
  const [settings] = useSettings();
  const now = useSecondTick(snap);
  const [editing, setEditing] = useState<Editing>(null);
  const [flash, setFlash] = useState<string | null>(null);
  const widget = settings?.widget ?? "large";
  const { w, h } = LAYOUTS[widget];
  const scale = useStageScale(w, h, settings?.scale ?? 1);

  useEffect(() => {
    if (!flash) return;
    const t = window.setTimeout(() => setFlash(null), 4000);
    return () => clearTimeout(t);
  }, [flash]);

  // The keyboard handler is bound once, so it reads the latest model through a ref.
  const model = useRef<Model | null>(null);
  useKeyboard(snap, editing, setEditing, model);

  if (!snap || !settings) return null;

  const remaining = remainingOf(snap, now);
  const m: Model = {
    snap,
    settings,
    remaining,
    fraction: snap.durationMs > 0 ? remaining / snap.durationMs : 0,
    editing,
    setEditing,
    hideKey: prettyShortcut(settings.hideShortcut),
    now,
    flash,
    doneTask: () => {
      if (snap.taskId == null) return;
      setFlash(`✓ ${snap.task} · ${Math.floor(taskSpent(m) / MINUTE)}m`);
      act({ type: "doneTask" });
    },
  };
  model.current = m;

  const onMouseDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest("button, input, [data-nodrag]")) return;
    api.startDragging();
  };

  return (
    <div
      className={`clock-root ${clockStyleClass(settings.style)} layout-${widget}`}
      style={{ ["--op" as string]: settings.opacity }}
      onMouseDown={onMouseDown}
      onContextMenu={(e) => {
        e.preventDefault();
        api.showClockMenu();
      }}
    >
      <div className="stage" style={{ width: w, height: h, transform: `scale(${scale})` }}>
        <Layout m={m} />
        {isTauri && (
          <div
            className="grip"
            data-nodrag
            title="Resize"
            onMouseDown={(e) => {
              e.stopPropagation();
              api.startResizeDragging();
            }}
          />
        )}
      </div>
    </div>
  );
}

/**
 * Fit the stage to the window. After a drag-resize settles, report the new
 * scale so the window snaps back to the widget's shape and is remembered.
 */
function useStageScale(w: number, h: number, saved: number): number {
  const fit = () => Math.min(window.innerWidth / w, window.innerHeight / h);
  const [scale, setScale] = useState(fit);
  useLayoutEffect(() => {
    setScale(fit());
    let settle = 0;
    const onResize = () => {
      setScale(fit());
      clearTimeout(settle);
      settle = window.setTimeout(() => {
        const s = Math.round(fit() * 100) / 100;
        const offShape = Math.abs(window.innerHeight - window.innerWidth * (h / w)) > 2;
        if (offShape || Math.abs(s - saved) > 0.015) api.setClockScale(s);
      }, 450);
    };
    window.addEventListener("resize", onResize);
    return () => {
      clearTimeout(settle);
      window.removeEventListener("resize", onResize);
    };
  }, [w, h, saved]);
  return scale;
}

function useKeyboard(
  snap: Snapshot | null,
  editing: Editing,
  setEditing: (e: Editing) => void,
  model: React.RefObject<Model | null>,
) {
  useEffect(() => {
    if (!snap || editing) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey && e.key === ",") return void api.openPanel("settings");
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const step = (e.shiftKey ? 5 : 1) * MINUTE;
      const map: Record<string, () => void> = {
        Space: () => act({ type: "toggle" }),
        KeyR: () => act({ type: e.shiftKey ? "restart" : "reset" }),
        KeyS: () => act({ type: "skip" }),
        ArrowUp: () => act({ type: "adjust", value: step }),
        ArrowDown: () => act({ type: "adjust", value: -step }),
        KeyT: () => setEditing("task"),
        KeyD: () => model.current?.doneTask(),
        KeyN: () => act({ type: "nextTask" }),
        Enter: () => setEditing("time"),
      };
      const fn = map[e.code];
      if (fn) {
        e.preventDefault();
        fn();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [snap === null, editing]);
}
