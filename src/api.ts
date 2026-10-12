// Single doorway to the Rust core. Outside Tauri (plain `npm run dev` in a
// browser) a small in-memory mock stands in, so the UI can be previewed and
// screenshotted without the desktop shell. Mock state is seeded from the URL,
// e.g. ?view=clock&phase=focus&run=running&remaining=1122&task=Write

import type { Action, AppInfo, Bucket, EmailStatus, HelperStatus, InsightsView, ParsedTask, PeriodStats, Quick, Settings, Snapshot, TaskPatch, TaskRow, TodayStats } from "./types";
import { MINUTE } from "./format";

export const isTauri = "__TAURI_INTERNALS__" in window;
type Unlisten = () => void;

export interface Backend {
  view(): "clock" | "panel" | "quick";
  snapshot(): Promise<Snapshot>;
  act(action: Action): Promise<Snapshot>;
  /** Open tasks, plus tasks done in the last `doneDays` days. */
  tasksList(doneDays: number): Promise<TaskRow[]>;
  taskParse(text: string): Promise<ParsedTask | null>;
  taskAdd(text: string, bucket: Bucket, top: boolean): Promise<TaskRow>;
  taskUpdate(id: number, patch: TaskPatch): Promise<TaskRow>;
  /** `beforeId: null` puts it at the end of the bucket. */
  taskMove(id: number, bucket: Bucket, beforeId: number | null): Promise<void>;
  taskSetDone(id: number, done: boolean): Promise<void>;
  taskDelete(id: number): Promise<void>;
  taskUndoDelete(): Promise<TaskRow | null>;
  onTasks(cb: () => void): Unlisten;
  todayStats(): Promise<TodayStats>;
  periodStats(kind: "week" | "month", offset: number): Promise<PeriodStats>;
  recentProjects(): Promise<string[]>;
  insights(): Promise<InsightsView>;
  respondSuggestion(id: string, accept: boolean): Promise<InsightsView>;
  logDistraction(): Promise<void>;
  setDistractionCategory(id: number, category: string | null): Promise<void>;
  rateSession(id: number, rating: number | null, goal: string | null): Promise<void>;
  quickState(): Promise<Quick | null>;
  quickClose(): Promise<void>;
  helperStatus(): Promise<HelperStatus>;
  installDndShortcuts(): Promise<void>;
  emailStatus(): Promise<EmailStatus>;
  setEmailPassword(password: string): Promise<void>;
  sendTestEmail(): Promise<string>;
  previewEmail(): Promise<void>;
  openAppPasswords(): Promise<void>;
  onQuick(cb: (q: Quick) => void): Unlisten;
  onStats(cb: () => void): Unlisten;
  getSettings(): Promise<Settings>;
  setSettings(s: Settings): Promise<{ settings: Settings; shortcutError: string | null }>;
  toggleClock(): Promise<void>;
  openPanel(tab?: "today" | "plan" | "history" | "insights" | "settings"): Promise<void>;
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
    tasksList: (doneDays) => invoke("tasks_list", { doneDays }),
    taskParse: (text) => invoke("task_parse", { text }),
    taskAdd: (text, bucket, top) => invoke("task_add", { text, bucket, top }),
    taskUpdate: (id, patch) => invoke("task_update", { id, patch }),
    taskMove: (id, bucket, beforeId) => invoke("task_move", { id, bucket, beforeId }),
    taskSetDone: (id, done) => invoke("task_set_done", { id, done }),
    taskDelete: (id) => invoke("task_delete", { id }),
    taskUndoDelete: () => invoke("task_undo_delete"),
    onTasks: (cb) => listen("chronos://tasks", cb),
    todayStats: () => invoke("today_stats"),
    periodStats: (kind, offset) => invoke("period_stats", { kind, offset }),
    recentProjects: () => invoke("recent_projects"),
    insights: () => invoke("insights"),
    respondSuggestion: (id, accept) => invoke("respond_suggestion", { id, accept }),
    logDistraction: () => invoke("log_distraction"),
    setDistractionCategory: (id, category) => invoke("set_distraction_category", { id, category }),
    rateSession: (id, rating, goal) => invoke("rate_session", { id, rating, goal }),
    quickState: () => invoke("quick_state"),
    quickClose: () => invoke("quick_close"),
    helperStatus: () => invoke("helper_status"),
    installDndShortcuts: () => invoke("install_dnd_shortcuts"),
    emailStatus: () => invoke("email_status"),
    setEmailPassword: (password) => invoke("set_email_password", { password }),
    sendTestEmail: () => invoke("send_test_email"),
    previewEmail: () => invoke("preview_email"),
    openAppPasswords: () => invoke("open_app_passwords"),
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
    menuBarTimer: "off", distractionShortcut: "Control+Alt+D", reflection: true, notifications: true, companion: true,
    emailReport: q.has("email"), emailTo: q.has("email") ? "you@gmail.com" : "", emailFrom: q.has("email") ? "you@gmail.com" : "", emailDay: 0, emailHour: 9,
    sound: true, volume: 0.6, dailyGoalMin: 180,
  };
  const phase = (q.get("phase") as Snapshot["phase"]) ?? "focus";
  const run = (q.get("run") as Snapshot["run"]) ?? "idle";
  const duration = +(q.get("duration") ?? (phase === "focus" ? 25 : phase === "short_break" ? 5 : 15)) * MINUTE;
  const remaining = q.has("remaining") ? +q.get("remaining")! * 1000 : duration;
  let snap: Snapshot = {
    phase, run, durationMs: duration, remainingMs: remaining,
    endAt: run === "running" ? now() + remaining : null,
    cycle: +(q.get("cycle") ?? 2), longEvery: 4, task: q.get("task"), project: q.get("project"),
    taskId: null, taskEstimateMs: null, taskSpentMs: 0, todayOpen: 0, nextTask: null, preset: "standard",
    sessionNumber: 3, completedToday: 2, focusMsToday: 52 * MINUTE, goalMs: 180 * MINUTE,
    distractionsToday: 3, awayMs: q.has("away") ? +q.get("away")! * MINUTE : null, companion: q.get("companion"),
    hidden: false, now: now(),
  };
  const subs = new Set<(s: Snapshot) => void>();
  const settingSubs = new Set<(s: Settings) => void>();
  const taskSubs = new Set<() => void>();
  const remain = () => (snap.run === "running" && snap.endAt ? Math.max(0, snap.endAt - now()) : snap.remainingMs);

  // Planner tasks, as in the mockup. ?tasks=0 starts with an empty list.
  const t0 = now();
  let nextId = 1;
  let tasks: TaskRow[] = [];
  const seed = (bucket: Bucket, title: string, project: string | null, est: number | null, spent = 0, extra: Partial<TaskRow> = {}) =>
    tasks.push({
      id: nextId, title, project, bucket, position: nextId++, estimateMs: est && est * MINUTE, done: false,
      createdAt: t0, movedAt: t0, doneAt: null, spentMs: spent * MINUTE, leftover: false, ...extra,
    });
  if (q.get("tasks") !== "0") {
    seed("today", "Prep slides for capstone demo", "course", 60, 38);
    seed("today", "Tailor resume for AI PM role", "job-search", 45);
    seed("today", "Chronos: tasks table migration", "chronos", 90);
    seed("today", "Reply to recruiter emails", "job-search", 20, 0, { leftover: true, movedAt: t0 - 86_400_000 });
    seed("today", "Morning walk", "life", 30, 32, { done: true, doneAt: t0 - 3 * 3_600_000 });
    seed("week", "Mock interview practice", "job-search", 60);
    seed("week", "Write LinkedIn post about Chronos", "brand", 40);
    seed("week", "Weekly review", "life", null);
    seed("week", "Apply to 3 roles", "job-search", 90);
    seed("backlog", "Learn Rust error handling", "chronos", 120);
    seed("backlog", "Read Deep Work, chapter 4", "reading", 35);
    seed("backlog", "Clean up old notes", "life", null);
  }
  let deleted: TaskRow | null = null;
  const open = (b: Bucket) => tasks.filter((t) => t.bucket === b && !t.done).sort((a, c) => a.position - c.position);
  const find = (id: number) => tasks.find((t) => t.id === id);
  const live = () => (snap.taskId == null ? undefined : find(snap.taskId));
  const goLive = (t: TaskRow | undefined) => {
    snap = { ...snap, taskId: t?.id ?? null, task: t?.title ?? null, project: t?.project ?? null, taskEstimateMs: t?.estimateMs ?? null, taskSpentMs: t?.spentMs ?? 0 };
  };
  const place = (b: Bucket, beforeId: number | null, skip?: number) => {
    const col = tasks.filter((t) => t.bucket === b && t.id !== skip).sort((a, c) => a.position - c.position);
    const i = beforeId == null ? -1 : col.findIndex((t) => t.id === beforeId);
    if (i < 0) return (col[col.length - 1]?.position ?? 0) + 1;
    return i === 0 ? col[0].position - 1 : (col[i - 1].position + col[i].position) / 2;
  };
  const tasksChanged = () => {
    const l = live();
    if (snap.taskId != null && (!l || l.done || l.bucket !== "today")) goLive(open("today")[0]);
    else if (l) goLive(l);
    snap = { ...snap, todayOpen: open("today").length };
    taskSubs.forEach((cb) => cb());
    emit();
  };
  // A port of tasks::parse (Rust) for previews: first #tag is the project,
  // the longest trailing run of words that reads as a duration is the estimate.
  const UNITS: Record<string, "h" | "m"> = { h: "h", hr: "h", hrs: "h", hour: "h", hours: "h", m: "m", min: "m", mins: "m", minute: "m", minutes: "m" };
  const parseDuration = (s: string): number | null => {
    let rest = s.toLowerCase();
    let mins = 0;
    let last: "h" | "m" | null = null;
    while (rest) {
      if (last === "m") return null;
      const n = rest.match(/^[\d.,]+/);
      const v = n ? Number(n[0].replace(",", ".")) : NaN;
      if (!n || Number.isNaN(v)) return null;
      rest = rest.slice(n[0].length).trimStart();
      const word = rest.match(/^[a-z]*/)![0];
      const unit: "h" | "m" | null = UNITS[word] ?? (word === "" && rest === "" && last === "h" ? "m" : null);
      if (!unit || unit === last) return null;
      mins += unit === "h" ? v * 60 : v;
      last = unit;
      rest = rest.slice(word.length).trimStart();
    }
    return last ? Math.round(mins) : null;
  };
  const parse = (text: string): ParsedTask | null => {
    let project: string | null = null;
    const words: string[] = [];
    for (const w of text.split(/\s+/)) {
      if (!w.startsWith("#")) {
        if (w) words.push(w);
        continue;
      }
      const tag = w.slice(1).replace(/[^\p{L}\p{N}_-]/gu, "").slice(0, 32).toLowerCase();
      if (!project && tag) project = tag;
    }
    let k = words.findIndex((_, i) => parseDuration(words.slice(i).join(" ")) != null);
    let mins = k >= 0 ? parseDuration(words.slice(k).join(" ")) : null;
    if (mins == null || mins < 1 || mins > 480) {
      k = words.length;
      mins = null;
    }
    const title = words.slice(0, k).join(" ").slice(0, 120);
    return title ? { title, project, estimateMs: mins && mins * MINUTE } : null;
  };
  if (q.has("live")) goLive(open("today")[0]);
  snap = { ...snap, todayOpen: open("today").length };

  const emit = () => {
    const next = open("today").find((t) => t.id !== snap.taskId);
    snap = { ...snap, now: now(), taskSpentMs: live()?.spentMs ?? 0, nextTask: next?.title ?? null };
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
          // A new focus block picks up the top Today task, as the engine does.
          if (snap.run === "idle" && snap.phase === "focus" && snap.taskId == null) goLive(open("today")[0]);
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
        case "setTask": {
          const p = a.value ? parse(a.value) : null;
          if (!p) {
            goLive(undefined);
            break;
          }
          let t = open("today").find((x) => x.title.toLowerCase() === p.title.toLowerCase());
          if (!t) {
            seed("today", p.title, p.project, p.estimateMs && p.estimateMs / MINUTE, 0, { position: place("today", open("today")[0]?.id ?? null) });
            t = tasks[tasks.length - 1];
          }
          goLive(t);
          tasksChanged();
          break;
        }
        case "startTask": {
          const t = find(a.value);
          if (!t) break;
          if (t.bucket !== "today" || t.done) {
            t.position = place("today", open("today")[0]?.id ?? null, t.id);
            t.bucket = "today";
            t.done = false;
            t.doneAt = null;
          }
          goLive(t);
          snap = { ...snap, phase: "focus", run: "running", endAt: now() + (snap.phase === "focus" ? r : 25 * MINUTE) };
          tasksChanged();
          break;
        }
        case "doneTask": {
          const t = live();
          if (!t) break;
          t.done = true;
          t.doneAt = now();
          tasksChanged();
          break;
        }
        case "nextTask": {
          const t = live();
          if (!t) break;
          t.position = place("today", null, t.id);
          goLive(open("today").find((x) => x.id !== t.id) ?? t);
          tasksChanged();
          break;
        }
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
      weekScore: 78,
    }),
    insights: async () => mockInsights(day.getTime(), q.has("empty")),
    respondSuggestion: async () => ({ ...mockInsights(day.getTime(), false), suggestion: null }),
    periodStats: async (kind, offset) => period(kind, offset),
    tasksList: async (doneDays) => {
      const since = doneDays === 0 ? Infinity : day.getTime() - (doneDays - 1) * 86_400_000;
      return tasks.filter((t) => !t.done || (t.doneAt ?? 0) >= since).map((t) => ({ ...t }));
    },
    taskParse: async (text) => parse(text),
    taskAdd: async (text, bucket, top) => {
      const p = parse(text);
      if (!p) throw "A task needs a title.";
      const position = top ? place(bucket, open(bucket)[0]?.id ?? null) : place(bucket, null);
      seed(bucket, p.title, p.project, p.estimateMs && p.estimateMs / MINUTE, 0, { position });
      const t = tasks[tasks.length - 1];
      tasksChanged();
      return { ...t };
    },
    taskUpdate: async (id, patch) => {
      const t = find(id);
      if (!t) throw "That task no longer exists.";
      if (patch.title !== undefined) {
        const p = parse(patch.title);
        if (!p) throw "A task needs a title.";
        t.title = p.title;
        t.project = p.project ?? t.project;
        t.estimateMs = p.estimateMs ?? t.estimateMs;
      }
      if (patch.estimateMs !== undefined) t.estimateMs = patch.estimateMs;
      tasksChanged();
      return { ...t };
    },
    taskMove: async (id, bucket, beforeId) => {
      const t = find(id);
      if (!t) throw "That task no longer exists.";
      t.position = place(bucket, beforeId === id ? null : beforeId, id);
      if (t.bucket !== bucket) t.movedAt = now();
      t.bucket = bucket;
      t.leftover = t.leftover && bucket === "today";
      tasksChanged();
    },
    taskSetDone: async (id, done) => {
      const t = find(id);
      if (!t) throw "That task no longer exists.";
      t.done = done;
      t.doneAt = done ? now() : null;
      if (!done) t.position = place(t.bucket, open(t.bucket).find((x) => x.id !== id)?.id ?? null, id);
      tasksChanged();
    },
    taskDelete: async (id) => {
      deleted = find(id) ?? null;
      tasks = tasks.filter((t) => t.id !== id);
      tasksChanged();
    },
    taskUndoDelete: async () => {
      const t = deleted;
      deleted = null;
      if (!t) return null;
      tasks.push({ ...t, spentMs: 0 });
      tasksChanged();
      return { ...t, spentMs: 0 };
    },
    onTasks: (cb) => {
      taskSubs.add(cb);
      return () => taskSubs.delete(cb);
    },
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
    emailStatus: async () => ({ hasPassword: q.has("pw"), status: { lastWeek: null, lastSentAt: null, lastAttemptAt: null, lastError: null }, nextSend: Date.now() + 4 * 86_400_000 }),
    setEmailPassword: async () => {},
    sendTestEmail: async () => "Sent to you@gmail.com.",
    previewEmail: async () => {},
    openAppPasswords: async () => {},
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

function mockInsights(today: number, empty: boolean): InsightsView {
  const week = 7 * 86_400_000;
  const trend = [52, 58, null, 61, 66, 64, 71, 78].map((value, i) => ({ start: today - (7 - i) * week, value }));
  if (empty) return { score: null, previous: null, mover: null, trend: trend.map((t) => ({ ...t, value: null })), insights: [], suggestion: null, sample: 2 };
  const part = (key: string, label: string, points: number, max: number, detail: string) => ({ key, label, points, max, detail });
  return {
    score: {
      value: 78,
      parts: [
        part("focus", "Focus time", 18.3, 25, "2h 12m/day of 3h goal"),
        part("consistency", "Consistency", 20, 20, "4 of 4 days"),
        part("completion", "Finishing sessions", 17.4, 20, "13 of 15 finished"),
        part("distractions", "Staying on task", 9.4, 15, "1.5 distractions per focus hour"),
        part("breaks", "Taking breaks", 6.4, 10, "7 of 11 breaks taken"),
        part("quality", "Session quality", 7.3, 10, "avg rating 3.2 / 4"),
      ],
    },
    previous: { value: 69, parts: [] },
    mover: ["Consistency", 6],
    trend,
    insights: [
      { id: "best-window", text: "You finish 27% more of your sessions when you start between 9 AM and 12 PM.", evidence: "92% vs 72% finished · 14 sessions in that window" },
      { id: "abandon-point", text: "When you stop a session early, it's usually around minute 38.", evidence: "median of 6 sessions ended early" },
      { id: "distraction-peak", text: "Your distractions peak between 2 PM and 4 PM.", evidence: "41% of 22 logged distractions" },
      { id: "length-trend", text: "Your average session grew from 31 to 42 minutes.", evidence: "last 2 weeks (18 sessions) vs the 2 before (15)" },
    ],
    suggestion: {
      id: "focus:25->30",
      text: "You've finished nearly every 25-minute session this week. Ready to try 30 minutes?",
      evidence: "12 of 13 finished",
      action: { label: "Switch to 30 min", setting: "focusMin", value: 30 },
    },
    sample: 41,
  };
}

export const api: Backend = isTauri ? tauriBackend() : mockBackend();
