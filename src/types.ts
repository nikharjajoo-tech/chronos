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
  preset: string;
  sessionNumber: number;
  completedToday: number;
  focusMsToday: number;
  goalMs: number;
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
  task: string | null;
}

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
