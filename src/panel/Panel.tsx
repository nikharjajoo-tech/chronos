import { useEffect, useState } from "react";
import { api } from "../api";
import { SettingsView } from "./SettingsView";
import { Today } from "./Today";
import "./panel.css";

type Tab = "today" | "settings";
const TABS: { id: Tab; label: string }[] = [
  { id: "today", label: "Today" },
  { id: "settings", label: "Settings" },
];
const tabFromHash = (): Tab => (location.hash.replace("#", "") === "settings" ? "settings" : "today");

export function Panel() {
  const [tab, setTab] = useState<Tab>(tabFromHash);

  useEffect(() => api.onTab((t) => setTab(t === "settings" ? "settings" : "today")), []);
  useEffect(() => {
    history.replaceState(null, "", `#${tab}`);
    document.title = tab === "today" ? "Chronos · Today" : "Chronos · Settings";
  }, [tab]);

  return (
    <div className="panel">
      <header className="panel-head">
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
      <main className="panel-body">{tab === "today" ? <Today /> : <SettingsView />}</main>
    </div>
  );
}
