import React from "react";
import ReactDOM from "react-dom/client";
import { api, isTauri } from "./api";
import { Clock } from "./clock/Clock";
import { Panel } from "./panel/Panel";

const view = api.view();
document.documentElement.classList.add(`${view}-window`);
if (!isTauri && view === "clock") {
  // Browser preview: put the transparent clock on a desk-coloured backdrop.
  document.body.style.background = "linear-gradient(135deg, #55657a, #2f3a49)";
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{view === "clock" ? <Clock /> : <Panel />}</React.StrictMode>,
);
