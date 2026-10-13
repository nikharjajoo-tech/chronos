// Mirrors the serde types in src-tauri (camelCase).

export type Phase = "focus" | "short_break" | "long_break";
export type RunState = "idle" | "running" | "paused";
export type Widget = "pill" | "small" | "medium" | "large";
export type ClockStyle = "retro" | "minimal" | "glass";
export type Theme = "glass" | "retro";

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
  /** The live planner task; null = free focus. */
  taskId: number | null;
  taskEstimateMs: number | null;
  /** All focus time on the live task as of `now`; add Date.now() - now while focus runs. */
  taskSpentMs: number;
  /** Open tasks in Today. */
  todayOpen: number;
  /** The Today task after the live one. */
  nextTask: string | null;
  preset: string;
  sessionNumber: number;
  completedToday: number;
  focusMsToday: number;
  goalMs: number;
  distractionsToday: number;
  /** Set when focus was auto-paused because you stepped away. */
  awayMs: number | null;
  /** A rare one-line nudge after a session ends. */
  companion: string | null;
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
  | { type: "preset"; value: string }
  | { type: "startTask"; value: number }
  | { type: "doneTask" }
  | { type: "nextTask" };

export type Bucket = "today" | "week" | "backlog";
export type PlanMode = "panel" | "board";

export interface TaskRow {
  id: number;
  title: string;
  project: string | null;
  bucket: Bucket;
  position: number;
  estimateMs: number | null;
  done: boolean;
  createdAt: number;
  movedAt: number;
  doneAt: number | null;
  /** Focus time credited so far, including the running block. */
  spentMs: number;
  /** Open Today task carried over from an earlier day. */
  leftover: boolean;
}

/** The morning sheet. Everything open in Today starts ticked. */
export interface MorningState {
  /** Waiting to be answered. */
  open: boolean;
  today: TaskRow[];
  week: TaskRow[];
  /** The top few backlog tasks. */
  backlog: TaskRow[];
}

/** What `task_parse` made of typed text. */
export interface ParsedTask {
  title: string;
  project: string | null;
  estimateMs: number | null;
}

/** A task edit. `title` is re-parsed; `estimateMs: null` clears the estimate. */
export interface TaskPatch {
  title?: string;
  estimateMs?: number | null;
}

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
  /** Panel, board and their pop-ups. Light or dark follows macOS. */
  theme: Theme;
  scale: number;
  launchAtLogin: boolean;
  showInDock: boolean;
  awayMin: number;
  autoHide: boolean;
  autoHideApps: string[];
  dnd: boolean;
  menuBarTimer: "off" | "hidden" | "always";
  distractionShortcut: string;
  /** Opens the add-task popup from any app. */
  quickAddShortcut: string;
  /** Ask what goes into Today on the first activity of the day. */
  morningPrompt: boolean;
  /** Not before this hour (0–12). */
  morningAfterHour: number;
  reflection: boolean;
  notifications: boolean;
  companion: boolean;
  emailReport: boolean;
  emailTo: string;
  emailFrom: string;
  /** 0 = Monday … 6 = Sunday */
  emailDay: number;
  emailHour: number;
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
  weekScore: number | null;
}

export interface ScorePart {
  key: string;
  label: string;
  points: number;
  max: number;
  detail: string;
}

export interface Score {
  value: number;
  parts: ScorePart[];
}

export interface Insight {
  id: string;
  text: string;
  evidence: string;
}

export interface Suggestion {
  id: string;
  text: string;
  evidence: string;
  action: { label: string; setting: string; value: unknown } | null;
}

export interface InsightsView {
  score: Score | null;
  previous: Score | null;
  mover: [string, number] | null;
  trend: { start: number; value: number | null }[];
  insights: Insight[];
  suggestion: Suggestion | null;
  sample: number;
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
  | { kind: "reflection"; sessionId: number; minutes: number; task: string | null; project: string | null }
  | { kind: "addTask" };

export interface EmailStatus {
  hasPassword: boolean;
  status: { lastWeek: string | null; lastSentAt: number | null; lastAttemptAt: number | null; lastError: string | null };
  nextSend: number;
}

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
