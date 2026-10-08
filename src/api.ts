// Single doorway to the Rust core. Outside Tauri (plain `npm run dev` in a
// browser) a small in-memory mock stands in, so the UI can be previewed and
// screenshotted without the desktop shell. Mock state is seeded from the URL,
// e.g. ?view=clock&phase=focus&run=running&remaining=1122&task=Write

import type { Action, AppInfo, HelperStatus, PeriodStats, Quick, Settings, Snapshot, TodayStats } from "./types";
import { MINUTE } from "./format";

export const isTauri = "__TAURI_INTERNALS__" in window;
type Unlisten = () => void;

export interface Backend {
  view(): "clock" | "panel" | "quick";
  snapshot(): Promise<Snapshot>;
  act(action: Action): Promise<Snapshot>;
  todayStats(): Promise<TodayStats>;
  periodStats(kind: "week" | "month", offset: number): Promise<PeriodStats>;
  recentProjects(): Promise<string[]>;
  logDistraction(): Promise<void>;
  setDistractionCategory(id: number, category: string | null): Promise<void>;
  rateSession(id: number, rating: number | null, goal: string | null): Promise<void>;
  quickState(): Promise<Quick | null>;
  quickClose(): Promise<void>;
  helperStatus(): Promise<HelperStatus>;
  installDndShortcuts(): Promise<void>;
  onQuick(cb: (q: Quick) => void): Unlisten;
  onStats(cb: () => void): Unlisten;
  getSettings(): Promise<Settings>;
  setSettings(s: Settings): Promise<{ settings: Settings; shortcutError: string | null }>;
  toggleClock(): Promise<void>;
  openPanel(tab?: "today" | "history" | "settings"): Promise<void>;
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
    view: () => (label === "panel" || label === "quick" ? label : "clock"),
    snapshot: () => invoke("snapshot"),
    act: (action) => invoke("act", { action }),
    todayStats: () => invoke("today_stats"),
    periodStats: (kind, offset) => invoke("period_stats", { kind, offset }),
    recentProjects: () => invoke("recent_projects"),
    logDistraction: () => invoke("log_distraction"),
    setDistractionCategory: (id, category) => invoke("set_distraction_category", { id, category }),
    rateSession: (id, rating, goal) => invoke("rate_session", { id, rating, goal }),
    quickState: () => invoke("quick_state"),
    quickClose: () => invoke("quick_close"),
    helperStatus: () => invoke("helper_status"),
    installDndShortcuts: () => invoke("install_dnd_shortcuts"),
    onQuick: (cb) => listen("chronos://quick", cb),
    onStats: (cb) => listen("chronos://stats", cb),
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
    awayMin: 5, autoHide: true, autoHideApps: ["CptHost", "screencaptureui", "obs"], dnd: false,
    menuBarTimer: "off", distractionShortcut: "Control+Alt+D", reflection: true, notifications: true,
    sound: true, volume: 0.6, dailyGoalMin: 180,
  };
  const phase = (q.get("phase") as Snapshot["phase"]) ?? "focus";
  const run = (q.get("run") as Snapshot["run"]) ?? "idle";
  const duration = +(q.get("duration") ?? (phase === "focus" ? 25 : phase === "short_break" ? 5 : 15)) * MINUTE;
  const remaining = q.has("remaining") ? +q.get("remaining")! * 1000 : duration;
  let snap: Snapshot = {
    phase, run, durationMs: duration, remainingMs: remaining,
    endAt: run === "running" ? now() + remaining : null,
    cycle: +(q.get("cycle") ?? 2), longEvery: 4, task: q.get("task"), project: q.get("project"), preset: "standard",
    sessionNumber: 3, completedToday: 2, focusMsToday: 52 * MINUTE, goalMs: 180 * MINUTE,
    distractionsToday: 3, awayMs: q.has("away") ? +q.get("away")! * MINUTE : null,
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
  const row = (id: number, kind: Snapshot["phase"], status: "completed" | "abandoned", h: number, m: number, mins: number, pauses = 0, task: string | null = null, project: string | null = null, rating: number | null = null) => ({
    id, kind, status, startedAt: at(h, m), endedAt: at(h, m + mins), plannedMs: 25 * MINUTE, actualMs: mins * MINUTE,
    pauseCount: pauses, awayCount: 0, task, project, rating, goalCompleted: null,
  });
  const sessions = [
    row(1, "focus", "completed", 9, 5, 25, 0, "Finish AI PM resume", "job-search", 4),
    row(2, "short_break", "completed", 9, 30, 5),
    row(3, "focus", "completed", 9, 36, 47, 1, "Finish AI PM resume", "job-search", 3),
    row(4, "short_break", "completed", 10, 24, 6),
    row(5, "focus", "abandoned", 11, 2, 12, 2, "Inbox"),
    row(6, "focus", "completed", 13, 40, 25, 0, "Case study draft", "portfolio", 3),
    row(7, "short_break", "completed", 14, 5, 5),
    row(8, "focus", "completed", 14, 12, 25, 0, "Case study draft", "portfolio"),
  ] as TodayStats["sessions"];
  const distraction = (id: number, h: number, m: number, category: string | null) => ({ id, at: at(h, m), category, sessionId: null });

  // A month of plausible history for the History tab.
  const seeded = (i: number) => ((Math.sin(i * 12.9898) * 43758.5453) % 1 + 1) % 1;
  const period = (kind: "week" | "month", offset: number): PeriodStats => {
    const d0 = new Date(day);
    let first: Date;
    let n: number;
    if (kind === "week") {
      first = new Date(d0);
      first.setDate(d0.getDate() - ((d0.getDay() + 6) % 7) + offset * 7);
      n = 7;
    } else {
      first = new Date(d0.getFullYear(), d0.getMonth() + offset, 1);
      n = new Date(first.getFullYear(), first.getMonth() + 1, 0).getDate();
    }
    const days = Array.from({ length: n }, (_, i) => {
      const start = new Date(first.getFullYear(), first.getMonth(), first.getDate() + i).getTime();
      const future = start > now();
      const r = seeded(start / 86_400_000);
      const weekend = [0, 6].includes(new Date(start).getDay());
      const completed = future ? 0 : Math.round(r * (weekend ? 2 : 7));
      return { start, future, completed, abandoned: future ? 0 : Math.round(seeded(start) * 2), focusMs: completed * 27 * MINUTE, distractions: future ? 0 : Math.round(seeded(start + 1) * 6) };
    });
    const sum = (k: "focusMs" | "completed" | "abandoned" | "distractions") => days.reduce((a, d) => a + d[k], 0);
    const elapsed = days.filter((d) => !d.future).length;
    const active = days.filter((d) => d.completed > 0).length;
    const best = days.reduce((b, d) => (d.focusMs > b.focusMs ? d : b), days[0]);
    const totals = {
      focusMs: sum("focusMs"), completed: sum("completed"), abandoned: sum("abandoned"), focusSessions: sum("completed") + sum("abandoned"),
      distractions: sum("distractions"), completionRate: sum("completed") / Math.max(1, sum("completed") + sum("abandoned")),
      avgSessionMs: sum("focusMs") / Math.max(1, sum("completed") + sum("abandoned")), avgDailyFocusMs: sum("focusMs") / Math.max(1, elapsed),
      activeDays: active, elapsedDays: elapsed, consistency: active / Math.max(1, elapsed), bestDay: best.start, bestDayMs: best.focusMs, avgRating: 3.2,
    };
    const previous = { ...totals, focusMs: totals.focusMs * 0.82, completed: Math.round(totals.completed * 0.85), distractions: Math.round(totals.distractions * 1.3), completionRate: totals.completionRate - 0.06, avgSessionMs: totals.avgSessionMs * 0.9, avgDailyFocusMs: totals.avgDailyFocusMs * 0.82, consistency: totals.consistency - 0.1 };
    const hourly = [0, 0, 0, 0, 0, 0, 0, 10, 40, 95, 110, 80, 30, 25, 60, 70, 45, 20, 10, 15, 25, 10, 0, 0].map((v) => v * MINUTE);
    return {
      from: days[0].start, to: days[n - 1].start + 86_400_000, days, totals, previous,
      projects: [{ name: "job-search", value: 9 * 60 * MINUTE }, { name: "portfolio", value: 5 * 60 * MINUTE }, { name: "", value: 2 * 60 * MINUTE }],
      categories: [{ name: "social", value: 8 }, { name: "youtube", value: 5 }, { name: "phone", value: 4 }, { name: "", value: 2 }],
      focusByHour: hourly, distractionsByHour: hourly.map((v, h) => (h >= 14 && h <= 16 ? 4 : v > 0 ? 1 : 0)),
      streak: { current: 4, best: 9, todayDone: true },
    };
  };

  return {
    view: () => (q.get("view") === "panel" || q.get("view") === "quick" ? (q.get("view") as "panel" | "quick") : "clock"),
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
      distractions: 3, streak: { current: 4, best: 9, todayDone: true },
      projects: [{ name: "job-search", value: 72 * MINUTE }, { name: "portfolio", value: 50 * MINUTE }, { name: "", value: 12 * MINUTE }],
      distractionList: [distraction(1, 9, 50, "phone"), distraction(2, 14, 20, "youtube"), distraction(3, 14, 31, null)],
    }),
    periodStats: async (kind, offset) => period(kind, offset),
    recentProjects: async () => ["job-search", "portfolio"],
    logDistraction: async () => {},
    setDistractionCategory: async () => {},
    rateSession: async () => {},
    quickState: async () =>
      q.get("quick") === "reflection"
        ? { kind: "reflection", sessionId: 1, minutes: 25, task: "Finish AI PM resume", project: "job-search" }
        : { kind: "distraction", id: 1, countToday: 3, sessionNumber: 3 },
    quickClose: async () => {},
    helperStatus: async () => ({ dndInstalled: false, running: [] }),
    installDndShortcuts: async () => {},
    onQuick: () => () => {},
    onStats: () => () => {},
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
