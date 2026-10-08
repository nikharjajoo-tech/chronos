import React from "react";
import ReactDOM from "react-dom/client";
import { api, isTauri } from "./api";
import { Clock } from "./clock/Clock";
import { Panel } from "./panel/Panel";
import { Quick } from "./quick/Quick";

const view = api.view();
document.documentElement.classList.add(`${view}-window`);
if (!isTauri && view !== "panel") {
  // Browser preview: put the transparent clock on a desk-coloured backdrop.
  document.body.style.background = "linear-gradient(135deg, #55657a, #2f3a49)";
  if (view === "quick") {
    // Pin the preview to the popup's real window size (headless Chrome won't go that narrow).
    const root = document.getElementById("root")!;
    root.style.cssText = `position:relative;width:330px;height:${new URLSearchParams(location.search).get("quick") === "reflection" ? 186 : 146}px`;
  }
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{view === "clock" ? <Clock /> : view === "quick" ? <Quick /> : <Panel />}</React.StrictMode>,
);
