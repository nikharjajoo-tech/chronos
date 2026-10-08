// Single doorway to the Rust core. Outside Tauri (plain `npm run dev` in a
// browser) a small in-memory mock stands in, so the UI can be previewed and
// screenshotted without the desktop shell. Mock state is seeded from the URL,
// e.g. ?view=clock&phase=focus&run=running&remaining=1122&task=Write

import type { Action, AppInfo, DayStats, Settings, Snapshot } from "./types";
import { MINUTE } from "./format";

export const isTauri = "__TAURI_INTERNALS__" in window;
type Unlisten = () => void;

export interface Backend {
  view(): "clock" | "panel";
  snapshot(): Promise<Snapshot>;
  act(action: Action): Promise<Snapshot>;
  todayStats(): Promise<DayStats>;
  getSettings(): Promise<Settings>;
  setSettings(s: Settings): Promise<{ settings: Settings; shortcutError: string | null }>;
  toggleClock(): Promise<void>;
  openPanel(tab?: "today" | "settings"): Promise<void>;
  showClockMenu(): Promise<void>;
  setClockScale(scale: number): Promise<void>;
  testSound(): Promise<void>;
  appInfo(): Promise<AppInfo>;
  revealDataDir(): Promise<void>;
  startDragging(): void;
  startResizeDragging(): void;
  onSnapshot(cb: (s: Snapshot) => void): Unlisten;
  onSettings(cb: (s: Settings) => void): Unlisten;
  onTab(cb: (tab: string) => void): Unlisten;
}

function tauriBackend(): Backend {
  // Imported lazily so the browser preview never touches Tauri internals.
  const core = import("@tauri-apps/api/core");
  const event = import("@tauri-apps/api/event");
  const win = import("@tauri-apps/api/window");
  const invoke = async <T>(cmd: string, args?: Record<string, unknown>) => (await core).invoke<T>(cmd, args);
  const label = (window as any).__TAURI_INTERNALS__?.metadata?.currentWindow?.label ?? "clock";

  const listen = <T>(name: string, cb: (p: T) => void): Unlisten => {
    let off: Unlisten | null = null;
    let cancelled = false;
    event.then((m) =>
      m.listen<T>(name, (e) => cb(e.payload)).then((u) => {
        if (cancelled) u();
        else off = u;
      }),
    );
    return () => {
      cancelled = true;
      off?.();
    };
  };

  return {
    view: () => (label === "panel" ? "panel" : "clock"),
    snapshot: () => invoke("snapshot"),
    act: (action) => invoke("act", { action }),
    todayStats: () => invoke("today_stats"),
    getSettings: () => invoke("get_settings"),
    setSettings: (settings) => invoke("set_settings", { settings }),
    toggleClock: () => invoke("toggle_clock"),
    openPanel: (tab) => invoke("open_panel", { tab: tab ?? null }),
    showClockMenu: () => invoke("show_clock_menu"),
    setClockScale: (scale) => invoke("set_clock_scale", { scale }),
    testSound: () => invoke("test_sound"),
    appInfo: () => invoke("app_info"),
    revealDataDir: () => invoke("reveal_data_dir"),
    startDragging: () => void win.then((m) => m.getCurrentWindow().startDragging()),
    startResizeDragging: () => void win.then((m) => m.getCurrentWindow().startResizeDragging("SouthEast")),
    onSnapshot: (cb) => listen("chronos://snapshot", cb),
    onSettings: (cb) => listen("chronos://settings", cb),
    onTab: (cb) => listen("chronos://tab", cb),
  };
}

// ── browser mock ────────────────────────────────────────────────────────────

function mockBackend(): Backend {
  const q = new URLSearchParams(location.search);
  const now = () => Date.now();
  let settings: Settings = {
    preset: "standard", focusMin: 25, shortMin: 5, longMin: 15, longEvery: 4,
    autoStartBreaks: false, autoStartFocus: false, alwaysOnTop: true, allWorkspaces: true,
    opacity: 1, captureProtected: false, hideShortcut: "CommandOrControl+Shift+H",
    widget: (q.get("widget") as Settings["widget"]) ?? "large", style: (q.get("style") as Settings["style"]) ?? "retro",
    scale: 1, launchAtLogin: true, showInDock: false,
    sound: true, volume: 0.6, dailyGoalMin: 180,
  };
  const phase = (q.get("phase") as Snapshot["phase"]) ?? "focus";
  const run = (q.get("run") as Snapshot["run"]) ?? "idle";
  const duration = +(q.get("duration") ?? (phase === "focus" ? 25 : phase === "short_break" ? 5 : 15)) * MINUTE;
  const remaining = q.has("remaining") ? +q.get("remaining")! * 1000 : duration;
  let snap: Snapshot = {
    phase, run, durationMs: duration, remainingMs: remaining,
    endAt: run === "running" ? now() + remaining : null,
    cycle: +(q.get("cycle") ?? 2), longEvery: 4, task: q.get("task"), preset: "standard",
    sessionNumber: 3, completedToday: 2, focusMsToday: 52 * MINUTE, goalMs: 180 * MINUTE,
    hidden: false, now: now(),
  };
  const subs = new Set<(s: Snapshot) => void>();
  const settingSubs = new Set<(s: Settings) => void>();
  const remain = () => (snap.run === "running" && snap.endAt ? Math.max(0, snap.endAt - now()) : snap.remainingMs);
  const emit = () => {
    snap = { ...snap, now: now() };
    subs.forEach((cb) => cb(snap));
    return snap;
  };

  const day = new Date();
  day.setHours(0, 0, 0, 0);
  const at = (h: number, m: number) => day.getTime() + (h * 60 + m) * MINUTE;
  const row = (id: number, kind: Snapshot["phase"], status: "completed" | "abandoned", h: number, m: number, mins: number, pauses = 0, task: string | null = null) => ({
    id, kind, status, startedAt: at(h, m), endedAt: at(h, m + mins), plannedMs: 25 * MINUTE, actualMs: mins * MINUTE, pauseCount: pauses, task,
  });
  const sessions = [
    row(1, "focus", "completed", 9, 5, 25, 0, "Finish AI PM resume"),
    row(2, "short_break", "completed", 9, 30, 5),
    row(3, "focus", "completed", 9, 36, 47, 1, "Finish AI PM resume"),
    row(4, "short_break", "completed", 10, 24, 6),
    row(5, "focus", "abandoned", 11, 2, 12, 2, "Inbox"),
    row(6, "focus", "completed", 13, 40, 25, 0, "Case study draft"),
    row(7, "short_break", "completed", 14, 5, 5),
    row(8, "focus", "completed", 14, 12, 25, 0, "Case study draft"),
  ] as DayStats["sessions"];

  return {
    view: () => (q.get("view") === "panel" ? "panel" : "clock"),
    snapshot: async () => emit(),
    act: async (a) => {
      const r = remain();
      switch (a.type) {
        case "toggle":
          snap = snap.run === "running"
            ? { ...snap, run: "paused", remainingMs: r, endAt: null }
            : { ...snap, run: "running", endAt: now() + r };
          break;
        case "reset":
          snap = { ...snap, run: "idle", remainingMs: snap.durationMs, endAt: null };
          break;
        case "adjust":
        case "setRemaining": {
          const next = Math.max(1000, a.type === "adjust" ? r + a.value : a.value);
          snap = { ...snap, durationMs: snap.durationMs + (next - r), remainingMs: next, endAt: snap.run === "running" ? now() + next : null };
          break;
        }
        case "setTask":
          snap = { ...snap, task: a.value };
          break;
        default:
          break;
      }
      return emit();
    },
    todayStats: async () => ({
      focusSessions: 5, completed: 4, abandoned: 1, interrupted: 2,
      focusMs: 134 * MINUTE, breakMs: 16 * MINUTE, avgFocusMs: 27 * MINUTE, longestFocusMs: 47 * MINUTE,
      currentStreak: 2, firstStart: sessions[0].startedAt, lastEnd: sessions[sessions.length - 1].endedAt, sessions,
    }),
    getSettings: async () => settings,
    setSettings: async (s) => {
      settings = s;
      settingSubs.forEach((cb) => cb(settings));
      return { settings, shortcutError: null };
    },
    toggleClock: async () => {},
    openPanel: async () => {},
    showClockMenu: async () => {},
    setClockScale: async () => {},
    testSound: async () => {},
    appInfo: async () => ({ version: "0.1.0", dataDir: "~/Library/Application Support/com.chronos.desk" }),
    revealDataDir: async () => {},
    startDragging: () => {},
    startResizeDragging: () => {},
    onSnapshot: (cb) => {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    onSettings: (cb) => {
      settingSubs.add(cb);
      return () => settingSubs.delete(cb);
    },
    onTab: () => () => {},
  };
}

export const api: Backend = isTauri ? tauriBackend() : mockBackend();
