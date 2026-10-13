import { useEffect, useState } from "react";
import { api } from "../api";
import { useSettings, useTheme } from "../hooks";
import type { MorningState, PlanMode } from "../types";
import { Morning } from "./Morning";
import { History } from "./History";
import { Insights } from "./Insights";
import { Plan } from "./Plan";
import { SettingsView } from "./SettingsView";
import { Today } from "./Today";
import "./panel.css";

type Tab = "today" | "plan" | "history" | "insights" | "settings";
const TABS: { id: Tab; label: string }[] = [
  { id: "today", label: "Today" },
  { id: "plan", label: "Plan" },
  { id: "history", label: "History" },
  { id: "insights", label: "Insights" },
  { id: "settings", label: "Settings" },
];
const asTab = (t: string): Tab => (TABS.some((x) => x.id === t) ? (t as Tab) : "today");
const tabFromHash = (): Tab => asTab(location.hash.replace("#", ""));

export function Panel() {
  const [tab, setTab] = useState<Tab>(tabFromHash);
  const [settings] = useSettings();
  useTheme(settings);
  const [planMode, setPlanModeState] = useState<PlanMode | null>(null);
  const [morning, setMorning] = useState<MorningState | null>(null);
  // The morning sheet always opens over the board.
  const board = tab === "plan" && (planMode === "board" || morning !== null);

  useEffect(() => {
    const check = () =>
      api.morningState().then((m) => {
        if (!m.open) return;
        setTab("plan");
        setMorning(m);
      });
    check();
    return api.onMorning(check);
  }, []);

  useEffect(() => void api.planMode().then(setPlanModeState), []);
  // Board view is the same window, widened. Other tabs always use panel size;
  // coming back to Plan widens it again because Plan remembers "board".
  useEffect(() => {
    if (planMode) api.setPanelLayout(board);
  }, [board, planMode !== null]);
  const setPlanMode = (m: PlanMode) => {
    setPlanModeState(m);
    api.setPlanMode(m);
  };

  useEffect(() => api.onTab((t) => setTab(asTab(t))), []);
  useEffect(() => {
    // In-page links like <a href="#insights"> switch tabs too.
    const onHash = () => setTab(tabFromHash());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);
  useEffect(() => {
    history.replaceState(null, "", `#${tab}`);
    document.title = `Chronos · ${TABS.find((t) => t.id === tab)!.label}`;
  }, [tab]);

  return (
    <div className={`panel${board ? " board-layout" : ""}`}>
      <header className="panel-head" data-tauri-drag-region>
        <div className="logo">
          <span className="logo-mark" aria-hidden />
          CHRONOS
        </div>
        <nav className="tabs">
          {TABS.map((t) => (
            <button key={t.id} className={tab === t.id ? "on" : ""} onClick={() => setTab(t.id)}>
              {t.label}
            </button>
          ))}
        </nav>
      </header>
      <main className="panel-body">{tab === "today" ? <Today /> : tab === "plan" ? planMode && <Plan mode={morning ? "board" : planMode} setMode={setPlanMode} /> : tab === "history" ? <History /> : tab === "insights" ? <Insights /> : <SettingsView />}</main>
      {morning && tab === "plan" && (
        <Morning
          state={morning}
          onStart={() => {
            // Back to the panel, with the top Today task ready on the clock.
            setPlanMode("panel");
            setMorning(null);
          }}
          onNotToday={() => {
            setPlanMode("board");
            setMorning(null);
          }}
        />
      )}
    </div>
  );
}
