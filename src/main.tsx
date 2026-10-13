import React from "react";
import ReactDOM from "react-dom/client";
import { api, isTauri } from "./api";
import { cachedTheme } from "./hooks";
import { Clock } from "./clock/Clock";
import { Panel } from "./panel/Panel";
import { Quick } from "./quick/Quick";

const view = api.view();
document.documentElement.classList.add(`${view}-window`);
if (isTauri) document.documentElement.classList.add("tauri");
if (view === "panel") document.documentElement.dataset.theme = cachedTheme();
const previewParams = new URLSearchParams(location.search);
if (!isTauri && view !== "panel") {
  // Browser preview: put the transparent clock on a desk-coloured backdrop
  // (unless embedded with ?bare, e.g. in the README showcase).
  if (!previewParams.has("bare")) document.body.style.background = "linear-gradient(135deg, #55657a, #2f3a49)";
  // Embedded stills: no transitions, so a screenshot never catches a digit mid-fade.
  else document.head.insertAdjacentHTML("beforeend", "<style>*,*::before,*::after{transition:none!important;animation:none!important}</style>");
  if (view === "quick") {
    // Pin the preview to the popup's real window size (headless Chrome won't go that narrow).
    const root = document.getElementById("root")!;
    root.style.cssText = `position:relative;width:330px;height:${previewParams.get("quick") === "reflection" ? 186 : 146}px`;
  }
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{view === "clock" ? <Clock /> : view === "quick" ? <Quick /> : <Panel />}</React.StrictMode>,
);
