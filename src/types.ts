// Mirrors the serde types in src-tauri (camelCase).

export type Phase = "focus" | "short_break" | "long_break";
export type RunState = "idle" | "running" | "paused";
export type Widget = "pill" | "small" | "medium" | "large";
export type ClockStyle = "retro" | "minimal";

export interface Snapshot {
  phase: Phase;
  run: RunState;
  durationMs: number;
  remainingMs: number;
  /** Epoch ms; set only while running. Remaining = endAt - Date.now(). */
  endAt: number | null;
  cycle: number;
  longEvery: number;
  task: string | null;
  project: string | null;
  preset: string;
  sessionNumber: number;
  completedToday: number;
  focusMsToday: number;
  goalMs: number;
  distractionsToday: number;
  /** Set when focus was auto-paused because you stepped away. */
  awayMs: number | null;
  hidden: boolean;
  now: number;
}

export type Action =
  | { type: "toggle" }
  | { type: "reset" }
  | { type: "restart" }
  | { type: "skip" }
  | { type: "adjust"; value: number }
  | { type: "setRemaining"; value: number }
  | { type: "switchPhase"; value: Phase }
  | { type: "setTask"; value: string | null }
  | { type: "preset"; value: string };

export interface Settings {
  preset: string;
  focusMin: number;
  shortMin: number;
  longMin: number;
  longEvery: number;
  autoStartBreaks: boolean;
  autoStartFocus: boolean;
  alwaysOnTop: boolean;
  allWorkspaces: boolean;
  opacity: number;
  captureProtected: boolean;
  hideShortcut: string;
  widget: Widget;
  style: ClockStyle;
  scale: number;
  launchAtLogin: boolean;
  showInDock: boolean;
  awayMin: number;
  autoHide: boolean;
  autoHideApps: string[];
  dnd: boolean;
  menuBarTimer: "off" | "hidden" | "always";
  distractionShortcut: string;
  reflection: boolean;
  notifications: boolean;
  sound: boolean;
  volume: number;
  dailyGoalMin: number;
}

export interface SessionRow {
  id: number;
  kind: Phase;
  status: "completed" | "abandoned";
  startedAt: number;
  endedAt: number | null;
  plannedMs: number;
  actualMs: number;
  pauseCount: number;
  awayCount: number;
  task: string | null;
  project: string | null;
  rating: number | null;
  goalCompleted: "yes" | "partial" | "no" | null;
}

export interface DistractionRow {
  id: number;
  at: number;
  category: string | null;
  sessionId: number | null;
}

export interface Named {
  name: string;
  value: number;
}

export interface Streak {
  current: number;
  best: number;
  todayDone: boolean;
}

export interface TodayStats extends DayStats {
  streak: Streak;
  projects: Named[];
  distractionList: DistractionRow[];
}

export interface DayAgg {
  start: number;
  focusMs: number;
  completed: number;
  abandoned: number;
  distractions: number;
  future: boolean;
}

export interface Totals {
  focusMs: number;
  focusSessions: number;
  completed: number;
  abandoned: number;
  distractions: number;
  completionRate: number;
  avgSessionMs: number;
  avgDailyFocusMs: number;
  activeDays: number;
  elapsedDays: number;
  consistency: number;
  bestDay: number | null;
  bestDayMs: number;
  avgRating: number | null;
}

export interface PeriodStats {
  from: number;
  to: number;
  days: DayAgg[];
  totals: Totals;
  previous: Totals;
  projects: Named[];
  categories: Named[];
  focusByHour: number[];
  distractionsByHour: number[];
  streak: Streak;
}

export type Quick =
  | { kind: "distraction"; id: number; countToday: number; sessionNumber: number | null }
  | { kind: "reflection"; sessionId: number; minutes: number; task: string | null; project: string | null };

export interface HelperStatus {
  dndInstalled: boolean;
  running: string[];
}

export const DISTRACTION_CATEGORIES: { id: string; label: string; short: string; key: string }[] = [
  { id: "social", label: "Social media", short: "Social", key: "1" },
  { id: "youtube", label: "YouTube", short: "YouTube", key: "2" },
  { id: "phone", label: "Phone", short: "Phone", key: "3" },
  { id: "messaging", label: "Messaging", short: "Messages", key: "4" },
  { id: "browsing", label: "Browsing", short: "Browsing", key: "5" },
  { id: "work", label: "Work-related", short: "Work", key: "6" },
  { id: "other", label: "Other", short: "Other", key: "7" },
];

export interface DayStats {
  focusSessions: number;
  completed: number;
  abandoned: number;
  interrupted: number;
  focusMs: number;
  breakMs: number;
  avgFocusMs: number;
  longestFocusMs: number;
  currentStreak: number;
  firstStart: number | null;
  lastEnd: number | null;
  distractions: number;
  sessions: SessionRow[];
}

export interface AppInfo {
  version: string;
  dataDir: string;
}

export const PHASE_LABEL: Record<Phase, string> = {
  focus: "Focus",
  short_break: "Short Break",
  long_break: "Long Break",
};
