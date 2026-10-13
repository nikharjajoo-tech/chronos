// ⌃⌥T: a Spotlight-style box for adding a task from any app. It follows the
// panel theme (Glass or Retro), not the clock style. Enter adds, ⌘Enter adds
// and starts focus on it, Esc closes. Clicking away closes it too, keeping
// what you typed for next time.

import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { useSettings, useTheme } from "../hooks";
import type { Bucket } from "../types";
import { COLUMN_LABEL, estimateLabel, projectColor } from "../panel/planModel";
import { nextDest, useParsedTask, useShake } from "../panel/useTaskDraft";
import "./addtask.css";

export function AddTask() {
  const [settings] = useSettings();
  useTheme(settings);
  const [text, setText] = useState("");
  const [dest, setDest] = useState<Bucket>("today");
  const parsed = useParsedTask(text);
  const [bad, shake] = useShake();
  const input = useRef<HTMLInputElement>(null);
  const busy = useRef(false);

  // The window is reused: focus the box each time it is shown, and close
  // when you click into another app.
  useEffect(() => {
    const focus = () => {
      input.current?.focus();
      input.current?.select();
    };
    const blur = () => api.quickClose();
    focus();
    addEventListener("focus", focus);
    addEventListener("blur", blur);
    const off = api.onQuick(focus);
    return () => {
      removeEventListener("focus", focus);
      removeEventListener("blur", blur);
      off();
    };
  }, []);

  const close = (clear: boolean) => {
    if (clear) {
      setText("");
      setDest("today");
    }
    api.quickClose();
  };

  const submit = async (start: boolean) => {
    if (busy.current) return;
    if (!text.trim()) return shake();
    busy.current = true;
    try {
      const row = await api.taskAdd(text, dest, start);
      if (start) await api.act({ type: "startTask", value: row.id });
      close(true);
    } catch {
      shake();
    } finally {
      busy.current = false;
    }
  };

  const est = parsed && estimateLabel(parsed.estimateMs, 0);
  return (
    <div className="at-root">
      <div className={`at-card${bad ? " bad" : ""}`}>
        <label className="at-input">
          <PlusIcon />
          <span className="sr-only">New task</span>
          <input
            ref={input}
            value={text}
            maxLength={200}
            placeholder="New task   e.g. Prep slides 28min #job-search"
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                submit(e.metaKey);
              } else if (e.key === "Tab") {
                e.preventDefault();
                setDest(nextDest(dest));
              } else if (e.key === "Escape") {
                e.preventDefault();
                close(true);
              }
            }}
          />
        </label>
        <div className="at-chips">
          {parsed ? (
            <>
              <span className="at-chip">{parsed.title}</span>
              {parsed.project && (
                <span className="at-chip">
                  <i style={{ background: projectColor(parsed.project) }} />#{parsed.project}
                </span>
              )}
              {est && <span className="at-chip">{est.text}</span>}
            </>
          ) : (
            <span className="at-hint">{text.trim() ? "Needs a title" : "Add a duration like 30m and a #project if you like"}</span>
          )}
          <span className="at-spacer" />
          <button className="at-dest" title="Tab changes the list" onClick={() => setDest(nextDest(dest))}>
            Add to {COLUMN_LABEL[dest]} <span aria-hidden>▾</span>
          </button>
        </div>
        <div className="at-foot">
          <span><b>⏎</b> Add</span>
          <span><b>⌘⏎</b> Add and start</span>
          <span><b>tab</b> Change list</span>
          <span className="at-spacer" />
          <span><b>esc</b> Close</span>
        </div>
      </div>
    </div>
  );
}

const PlusIcon = () => (
  <svg width="20" height="20" viewBox="0 0 20 20" aria-hidden>
    <circle cx="10" cy="10" r="8.5" fill="none" stroke="currentColor" strokeWidth="1.4" />
    <path d="M10 6v8M6 10h8" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
  </svg>
);
