import { useEffect, useState } from "react";
import { api } from "../api";
import { History } from "./History";
import { Insights } from "./Insights";
import { SettingsView } from "./SettingsView";
import { Today } from "./Today";
import "./panel.css";

type Tab = "today" | "history" | "insights" | "settings";
const TABS: { id: Tab; label: string }[] = [
  { id: "today", label: "Today" },
  { id: "history", label: "History" },
  { id: "insights", label: "Insights" },
  { id: "settings", label: "Settings" },
];
const asTab = (t: string): Tab => (TABS.some((x) => x.id === t) ? (t as Tab) : "today");
const tabFromHash = (): Tab => asTab(location.hash.replace("#", ""));

export function Panel() {
  const [tab, setTab] = useState<Tab>(tabFromHash);

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
      <main className="panel-body">{tab === "today" ? <Today /> : tab === "history" ? <History /> : tab === "insights" ? <Insights /> : <SettingsView />}</main>
    </div>
  );
}
