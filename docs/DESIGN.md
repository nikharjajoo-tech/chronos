# Chronos: design notes

Chronos is a small retro-futuristic desk clock that floats on the desktop and runs
Pomodoro sessions. It gets out of the way when asked and keeps a local record of
how you work. This document covers the decisions made before writing code.

---

## 1. Technology stack

| Layer | Choice | Why |
|---|---|---|
| Shell | **Tauri 2** (Rust) | Uses the system WebView, so there is no bundled Chromium. Idle memory is about 30–60 MB, versus 150+ MB for Electron. It provides tray, global shortcuts, always-on-top and transparent windows. |
| UI | **React 19 + TypeScript + Vite** | Fast iteration and good fit for a small stateful UI. No CSS framework: hand-written CSS with design tokens, because the clock is a bespoke object, not a dashboard. |
| Storage | **SQLite via `rusqlite` (bundled)** | Local, offline, a single file, and queryable for analytics. Lives in the OS app-data folder. |
| Window state | `tauri-plugin-window-state` | Remembers position and size across launches. |
| Hotkeys | `tauri-plugin-global-shortcut` | Hide/show works even when another app is focused. |
| Sound | `afplay` (macOS system sounds) | No bundled audio assets. Plays even when the clock window is hidden. |

The **timer engine lives in Rust**, not in the WebView. A hidden WebView can be
throttled or suspended by the OS (App Nap). The Rust core keeps running, so
sessions still complete, record and chime while the clock is hidden.

## 2. Architecture

```
┌──────────────────────── Rust core (src-tauri) ───────────────────────┐
│                                                                       │
│  timer.rs     Pure state machine. Wall-clock based: end_at = now + d  │
│               No I/O; every transition takes `now` → fully testable.  │
│                                                                       │
│  engine.rs    Owns Timer + Db behind a Mutex. Applies commands,       │
│               persists state, records sessions, emits snapshots,      │
│               runs a 500 ms watchdog that detects completion.         │
│                                                                       │
│  db.rs        SQLite schema + migrations (PRAGMA user_version),       │
│               session CRUD, settings, stats queries, task queries.    │
│                                                                       │
│  tasks.rs     Planner types and the one parser for typed tasks        │
│               ("Prep slides 28m #course" → title, project, estimate). │
│                                                                       │
│  companion.rs Watchdog loop: away pauses, auto-hide, DND, popups,     │
│               the morning prompt, the weekly email schedule.          │
│                                                                       │
│  settings.rs  Typed settings (serde defaults → forward compatible).   │
│                                                                       │
│  shell.rs     Windows, tray, native menus, global shortcut,           │
│               always-on-top, screen-capture protection, sound,        │
│               panel ⇄ board resizing.                                 │
└───────────────▲──────────────────────────────────┬────────────────────┘
                │ invoke(command)                  │ emit("chronos://snapshot")
┌───────────────┴──────────────────────────────────▼────────────────────┐
│ WebView (src)                                                         │
│   clock window  → Floating clock (renders snapshot, 1 Hz repaint)     │
│   panel window  → Today · Plan · History · Insights · Settings;       │
│                   widens into the 4-column board for planning         │
│   quick window  → distraction / reflection cards, ⌃⌥T add-task box    │
└───────────────────────────────────────────────────────────────────────┘
```

**Single source of truth.** The UI never counts down on its own. It receives a
snapshot (`phase`, `run`, `endAt`, `remainingMs`, ...) and computes
`endAt - Date.now()` on each repaint. Commands go to Rust, and Rust broadcasts
the new snapshot to every window.

Beside these sit `analytics.rs` (weekly and monthly aggregates), `insights.rs`
(score, insights, suggestions), `report.rs` (the weekly email) and `sysmac.rs`
(idle time, process list, Shortcuts). Task changes also emit
`chronos://tasks`, and every window showing tasks refetches on it.

**The live task.** The top open Today task goes live when a focus block
starts. The timer keeps `task_id` and the moment the current stretch began;
switching or finishing a task credits that stretch to it, and when the block
ends the credits are written to `session_tasks` in one transaction. Pauses and
away time are never credited. The planner design is in
[PLANNER-PHASE1.md](PLANNER-PHASE1.md).

## 3. Local database

File: `<app-data>/chronos.sqlite3`. Migrations are numbered and tracked with
`PRAGMA user_version`. Data never leaves the machine.

```sql
-- every timed block, focus or break                          (migrations 1–3)
CREATE TABLE sessions (
  id              INTEGER PRIMARY KEY,
  kind            TEXT NOT NULL,      -- focus | short_break | long_break
  status          TEXT NOT NULL,      -- running | completed | abandoned | cancelled
  started_at      INTEGER NOT NULL,   -- epoch ms
  ended_at        INTEGER,            -- epoch ms
  planned_ms      INTEGER NOT NULL,   -- final planned length (after +/- adjustments)
  actual_ms       INTEGER,            -- time actually running (pauses excluded)
  pause_count     INTEGER NOT NULL DEFAULT 0,
  adjust_ms       INTEGER NOT NULL DEFAULT 0, -- net time added/removed while running
  task            TEXT,               -- the task with the most time in this block
  preset          TEXT,               -- standard | deep | quick | custom
  rating          INTEGER,            -- 1..4 end-of-session reflection
  goal_completed  TEXT,               -- yes | partial | no
  created_at      INTEGER NOT NULL,
  project         TEXT,               -- 2: the #tag, lower-case
  away_count      INTEGER NOT NULL DEFAULT 0, -- 2: automatic away pauses
  task_id         INTEGER REFERENCES tasks(id) ON DELETE SET NULL -- 3
);
CREATE INDEX idx_sessions_started ON sessions(started_at);
CREATE INDEX idx_sessions_project ON sessions(project);
CREATE INDEX idx_sessions_task    ON sessions(task_id);

CREATE TABLE distractions (
  id          INTEGER PRIMARY KEY,
  at          INTEGER NOT NULL,
  category    TEXT,
  note        TEXT,
  session_id  INTEGER REFERENCES sessions(id) ON DELETE SET NULL
);
CREATE INDEX idx_distractions_at ON distractions(at);

-- 3: planner tasks
CREATE TABLE tasks (
  id           INTEGER PRIMARY KEY,
  title        TEXT    NOT NULL,              -- ≤ 120 chars, no #tag, no estimate
  project      TEXT,                          -- lower-case tag, ≤ 32 chars
  bucket       TEXT    NOT NULL,              -- today | week | backlog
  position     REAL    NOT NULL,              -- ascending order inside the bucket
  estimate_ms  INTEGER,                       -- NULL = no estimate (never zero)
  status       TEXT    NOT NULL DEFAULT 'open', -- open | done
  created_at   INTEGER NOT NULL,
  moved_at     INTEGER NOT NULL,              -- when it entered its current bucket
  done_at      INTEGER
);
CREATE INDEX idx_tasks_bucket ON tasks(status, bucket, position);
CREATE INDEX idx_tasks_done   ON tasks(done_at);

-- 3: focus time inside one session, split by task
CREATE TABLE session_tasks (
  session_id  INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  task_id     INTEGER NOT NULL REFERENCES tasks(id)    ON DELETE CASCADE,
  active_ms   INTEGER NOT NULL,
  PRIMARY KEY (session_id, task_id)
);
CREATE INDEX idx_session_tasks_task ON session_tasks(task_id);

-- typed JSON values: settings, timer_state, suggestions_dismissed,
-- plan_mode, panel_size, board_size, morning_shown, report_status
CREATE TABLE kv (key TEXT PRIMARY KEY, value TEXT NOT NULL);
```

**DailyStats is derived, not stored.** It is computed from `sessions` with
`date(started_at/1000,'unixepoch','localtime')`. A personal dataset is tiny,
and keeping no second copy means it can never disagree with `sessions`. A
`daily_stats` cache table can be added later if it is ever needed.

**Tasks**

* `status` is separate from `bucket`, so a reopened task goes back where it was.
* `position` is a REAL: a move lands halfway between its new neighbours, so
  one row changes. When a gap gets too small the bucket is renumbered.
* An open Today task with `moved_at` before today's midnight is a leftover
  ("from Fri").
* Time spent is never stored on the task. It is `SUM(session_tasks.active_ms)`
  plus whatever the running block has credited so far.
* Deleting a task keeps your history: `sessions.task` and `sessions.project`
  still hold the text, so History, Insights and the email are unchanged.

**Status semantics**

* `completed`: the timer reached zero.
* `abandoned`: ended early by reset, skip or switch after at least 60 s of run time.
* `cancelled`: ended within 60 s, treated as a mis-click and excluded from stats.
* *interrupted* is not a status. It is a completed or abandoned focus session
  with `pause_count > 0`.
* Focused minutes are the sum of `actual_ms` over focus sessions, whatever
  their status, because time actually focused counts.

## 4. Floating clock wireframe

```
            ┌─────────────────────────────────┐
            │ ◉ ◉                        ⊘ ⋯ │  ← status lamps · hide · menu
            │   ╭───────────────────────╮     │
            │  ╱ · · · · ╷ · · · · ·    ╲    │  ← dial: 60 ticks, amber progress
            │ │  ┌──── FOCUS ────┐        │   │     arc + stepping hand
            │ │  │ ▐ 24:37 ▌     │  ← 7-segment amber readout (ghost 88:88)
            │ │  └───────────────┘        │   │     hover: [-5] [+5] [+10]
            │  ╲  ● ● ◐ ○  SESSION 3     ╱    │     click digits: type minutes
            │   ╰───────────────────────╯     │     scroll: ±1 min
            │  ✎ Finish AI PM resume          │  ← task line (click to edit)
            │   [ ↺ ]   [  ▶  ]   [ ⏭ ]       │  ← reset · start/pause · skip
            └──╥───────────────────────────╥──┘
               ▔                           ▔      ← little feet
```

* Drag anywhere on the casing; a resize grip sits in the bottom-right corner.
  Content scales uniformly with the window.
* The `⋯` button (or a right-click) opens a **native menu**: switch phase,
  presets, +5, +10, −5, restart, always on top, hide, stats and settings, quit.
  Native menus can extend past the small window and cost nothing while closed.
* The phase changes the palette. Focus is amber, a short break is teal and a
  long break is violet.

## 5. MVP feature set (Phase 1)

1. Floating, transparent, draggable, resizable clock
2. Pomodoro engine: focus, short break and long break, with a long break every N
3. Start, pause, resume, reset, skip and restart
4. Live adjustment: +5, +10 and −5, set an exact time, switch phase without restarting
5. Presets (Standard 25/5, Deep Work 50/10, Quick 15/5) plus custom durations
6. Hide mode: the ⌘⇧H global shortcut (configurable), the tray menu and a button. The timer keeps running.
7. Always on top, opacity, and remembered position and size
8. Optional: hide the clock from screen recordings and screenshots
9. Session recording in SQLite, with crash and restart recovery of a running timer
10. Today panel: sessions, completed, interrupted, abandoned, focused and break
    time, average and longest focus, current streak, and a timeline
11. Optional completion chime with volume control (pulled forward from Phase 2,
    because a timer that ends silently while hidden is not usable)
12. Optional task line ("What are you working on?")

Phase 2 and Phase 3 follow the brief.

## 6. Technical risks

| Risk | Mitigation |
|---|---|
| A hidden or occluded WebView gets throttled, so completion would be missed | The timer lives in Rust, and a 500 ms watchdog uses wall-clock comparison. |
| Laptop sleep, freezes or CPU load cause drift | `end_at` is absolute epoch ms. On wake, the watchdog sees `now ≥ end_at` and completes with `ended_at = end_at`. Rust's `Instant` stops during macOS sleep, so it is deliberately **not** used. |
| A system clock change (manual change or NTP jump) | This is accepted. Wall time is what a Pomodoro is about. |
| Transparent windows on macOS need `macOSPrivateApi` | This is fine for a personally distributed app. It would block a Mac App Store release. |
| Floating over full-screen apps on macOS | Visible on all workspaces is set. Some full-screen apps still win. That is an OS limitation. |
| A global shortcut collides with another app | The shortcut is configurable. A failed registration shows in Settings instead of failing silently. |
| Content protection (hide from capture) | Respected by the classic capture APIs. Some newer ScreenCaptureKit recorders may ignore it, so it is offered as an extra, with Hide Mode as the guaranteed path. |
| Lost data on a crash mid-session | The session row is inserted at start and timer state is saved on every change. On launch, overdue sessions complete and orphans become abandoned. |
| Scope creep makes the clock distracting | The clock stays minimal. Everything detailed goes to the panel window. Repaints happen at 1 Hz and stop entirely while hidden. |
