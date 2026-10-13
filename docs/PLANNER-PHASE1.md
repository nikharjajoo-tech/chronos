# Chronos Planner, Phase 1: spec

Phase 1 adds a task list to Chronos and connects it to the clock. You plan
tasks in a Plan tab, the top task in Today becomes the clock's live task, and
pressing Done moves the next one in. It covers quick capture, the Plan tab
(with the panel ⇄ board toggle) and the morning prompt.

Mockups: <https://claude.ai/artifact/2QQo4PuXCfZ11wmG2hHW8E>

---

## 0. Scope

**In Phase 1**

| # | Feature | Summary |
|---|---|---|
| 1 | Tasks | Title, `#project`, estimate, bucket (Backlog / This week / Today), order, done |
| 2 | Quick add | Parses `Prep slides 28min #job-search`, both in the Plan tab and from any app with ⌃⌥T |
| 3 | Live task on the clock | The top Today task goes live; **Done** brings in the next one; focus time is credited to the task |
| 4 | Task progress | The clock and cards show `38 / 60m` (time spent vs estimate) |
| 5 | Plan tab | A new panel tab: one column at a time, with drag to reorder |
| 6 | Board toggle | ⤢ / ⌘E widens the same window into a 4-column board; pressing again shrinks it back |
| 7 | Morning prompt | On the first activity of the day, opens the board and asks what goes into Today |

**Not in Phase 1** (planned later, so Phase 1 leaves room for them)

| Later phase | Feature |
|---|---|
| 2 | Early/late verdicts, estimate-accuracy insights, time per task in History, logging time by hand |
| 3 | Due dates, recurring tasks, reminders, moving tasks between buckets automatically by date |
| 4 | MCP server so Claude can read and write tasks |
| Later | Subtasks, notes, links that open when a task goes live, Google Calendar import |

**Principles carried over from Chronos**

- The clock stays a small object. All planning happens in the panel.
- Rust is the only source of truth. The UI never computes task time itself;
  it renders snapshots (see `docs/DESIGN.md` §2).
- Local only. Tasks live in the same `chronos.sqlite3` file.
- A missing estimate is never treated as zero.

---

## 1. Data model

### 1.1 Migration 3

Appended to `MIGRATIONS` in `src-tauri/src/db.rs`. Migrations 1 and 2 are not
changed.

```sql
-- 3 — tasks, and focus time credited to them
CREATE TABLE tasks (
  id           INTEGER PRIMARY KEY,
  title        TEXT    NOT NULL,              -- ≤ 120 chars, no #tag, no estimate
  project      TEXT,                          -- lower-case tag, ≤ 32 chars
  bucket       TEXT    NOT NULL,              -- today | week | backlog
  position     REAL    NOT NULL,              -- ascending order inside the bucket
  estimate_ms  INTEGER,                       -- NULL = no estimate
  status       TEXT    NOT NULL DEFAULT 'open', -- open | done
  created_at   INTEGER NOT NULL,
  moved_at     INTEGER NOT NULL,              -- when it entered its current bucket
  done_at      INTEGER
);
CREATE INDEX idx_tasks_bucket ON tasks(status, bucket, position);
CREATE INDEX idx_tasks_done   ON tasks(done_at);

-- Focus time inside one session, split by task (a session can cover several).
CREATE TABLE session_tasks (
  session_id  INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  task_id     INTEGER NOT NULL REFERENCES tasks(id)    ON DELETE CASCADE,
  active_ms   INTEGER NOT NULL,
  PRIMARY KEY (session_id, task_id)
);
CREATE INDEX idx_session_tasks_task ON session_tasks(task_id);

ALTER TABLE sessions ADD COLUMN task_id INTEGER REFERENCES tasks(id) ON DELETE SET NULL;
CREATE INDEX idx_sessions_task ON sessions(task_id);
```

**Design notes**

- **`status` is separate from `bucket`.** A done task keeps its bucket, so
  reopening it puts it back where it was.
- **`position` is a REAL.** A move sets it halfway between its new
  neighbours, so one row changes. When a gap drops below `1e-6`, the bucket is
  renumbered `1.0, 2.0, …` in one transaction.
- **`moved_at`** detects leftovers. An open Today task with
  `moved_at < start of today` is a leftover from an earlier day.
- **Deleting a task keeps your history.** `sessions.task` and
  `sessions.project` still hold the text, so History, Insights, the weekly
  score and the email work as they do now. `session_tasks` rows for that task
  are removed by the cascade.
- **Time spent is never stored on the task.** It is
  `SUM(session_tasks.active_ms)` plus whatever the running block has credited
  so far (§3.3). One source, no drift.

### 1.2 Rust types (`src-tauri/src/tasks.rs`, new module)

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bucket { Today, Week, Backlog }

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRow {
    pub id: i64,
    pub title: String,
    pub project: Option<String>,
    pub bucket: Bucket,
    pub position: f64,
    pub estimate_ms: Option<i64>,
    pub done: bool,
    pub created_at: i64,
    pub moved_at: i64,
    pub done_at: Option<i64>,
    /// Focus time credited so far, including the running block.
    pub spent_ms: i64,
    /// Open Today task carried over from an earlier day.
    pub leftover: bool,
}

/// The result of parsing what the user typed.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedTask {
    pub title: String,
    pub project: Option<String>,
    pub estimate_ms: Option<i64>,
}
```

`tasks.rs` contains the pure logic: parsing, ordering and leftover rules. SQL
goes in `db.rs`, next to the existing queries.

---

## 2. Parsing what you type

One function, `tasks::parse(text) -> ParsedTask`, is used everywhere: the Plan
tab, the ⌃⌥T popup and the clock's task line. The UI's preview chips call it
through `task_parse`, so the rules exist in only one place.

**Rules, applied in order**

1. **Project:** the first `#tag` is the project. This reuses
   `engine::split_project`: lower-cased, `[a-z0-9_-]`, at most 32 chars.
   Later `#tags` are dropped, as they are today.
2. **Estimate:** one or two duration tokens **at the end** of the remaining
   text. A number inside a title ("Read 3 chapters", "Call at 5pm") is never
   taken as an estimate.
3. **Title:** what remains, whitespace collapsed, at most 120 chars. An empty
   title is rejected (the input shakes, like `TimeInput`'s `bad` state).

**Accepted estimate forms** (case-insensitive, space optional):

| Typed | Estimate |
|---|---|
| `28m` `28 min` `28mins` `28 minutes` | 28 min |
| `1h` `1hr` `1 hour` `2hrs` `2 hours` | 60 / 120 min |
| `1.5h` `1,5h` | 90 min |
| `1h30` `1h30m` `1h 30m` `2hr 15min` `1 hour 15 minutes` | 90 / 135 / 75 min |
| `90min` | 90 min |

The estimate is limited to 1 min ≤ est ≤ 8 h (`MAX_MINUTES * 2`). A value
outside that range is left in the title, not dropped.

**Test cases** (`tasks::tests::parse_*`):

```
"Prep slides 28min #job-search" → ("Prep slides", "job-search", 28m)
"#job-search Prep slides 1h"    → ("Prep slides", "job-search", 60m)
"Email campaign 2hr 15min"      → ("Email campaign", None, 135m)
"Read 3 chapters"               → ("Read 3 chapters", None, None)
"Call mom at 5pm"               → ("Call mom at 5pm", None, None)
"Write 2 posts 45m"             → ("Write 2 posts", None, 45m)
"Deep dive 0m"                  → ("Deep dive 0m", None, None)
"30m"                           → rejected (empty title)
```

---

## 3. Engine: the live task and task time

### 3.1 Timer state (additions to `timer::Timer`)

All new fields use `#[serde(default)]`, so a `timer_state` saved by the
current version still loads.

```rust
/// The task being worked on; None = free focus (no task).
pub task_id: Option<i64>,
/// Focus time this block has already credited to tasks: (task_id, ms).
pub task_ms: Vec<(i64, i64)>,
/// `active()` at the moment the current task took over.
pub task_since_ms: i64,
```

`Finished` gains `task_ms: Vec<(i64, i64)>`, which includes the last stretch.

**Crediting rule:** time is credited only while `phase == Focus`. When the
live task changes, or a focus block ends, the stretch since the last change is
credited to the outgoing task:

```
credit = timer.active(now) - task_since_ms
```

`active()` already excludes paused and away time, so pauses and away time
never count.

### 3.2 Choosing the live task

- **Starting a focus block:** if `task_id` is `None`, or points to a task
  that is no longer an open Today task, the top open Today task becomes live.
  If Today is empty, the block runs as free focus.
- **Breaks:** the live task stays set, but no time is credited. The clock
  shows it as *Up next*.
- **Reordering:** dragging tasks during a block **does not** change the live
  task. Only the events in §3.4 change it.
- **Moving the live task out of Today, or deleting it:** its stretch is
  credited first (unless it was deleted), then the next Today task becomes
  live.

### 3.3 Spent time on the live task

```
spent(task) = SUM(session_tasks.active_ms WHERE task_id = task)
            + Σ timer.task_ms for that task
            + (timer.active(now) - task_since_ms)   if it is live and phase is Focus
```

The snapshot sends `taskSpentMs` as of `snap.now`. Between snapshots the
clock adds `Date.now() - snap.now` while focus is running, the same way it
already derives `remaining` from `endAt`.

### 3.4 New actions (`engine::Action`)

These change the timer, so they go through `act` / `shell::dispatch` and every
window gets a new snapshot.

| Action | JSON | Behaviour |
|---|---|---|
| `StartTask(id)` | `{type:"startTask", value:7}` | Make task 7 live (credit the outgoing stretch). If it was in Week or Backlog, move it to the **top of Today**. If idle in Focus, start the block. If in a break, end the break (like `SwitchPhase(Focus)`) and start focus. If already running, keep running. |
| `DoneTask` | `{type:"doneTask"}` | Credit the stretch, mark the live task done, and make the **next open Today task** live. **The timer keeps running**, so the Pomodoro rhythm is not interrupted. If no task is left, run as free focus. |
| `NextTask` | `{type:"nextTask"}` | "Not now": credit the stretch, move the live task to the **bottom of Today**, and make the next one live. |
| `SetTask(text)` *(changed)* | as today | Typing on the clock (T). If the text exactly matches an open Today task title (case-insensitive), that task goes live. Otherwise a **new task is created at the top of Today** (with parsing) and goes live. An empty text clears the live task, so the block runs as free focus. |

**Recording a finished block** (`Db::finish_session`, one transaction):

1. Write the `session_tasks` rows from `Finished.task_ms` (same task: amounts
   summed).
2. Set `sessions.task_id`, `task` and `project` from the task with the
   **most** time in the block (on a tie, the later one). With no task, the
   current free-text behaviour stays. Existing analytics (projects breakdown,
   insights) keep working without changes.
3. `Cancelled` blocks credit nothing, as now. `Abandoned` blocks keep their
   credit, because that was real work.

### 3.5 Snapshot additions

```rust
pub task_id: Option<i64>,
pub task_estimate_ms: Option<i64>,
/// All focus time on the live task, as of `now` (§3.3).
pub task_spent_ms: i64,
/// Open tasks in Today, for the "list is clear" message.
pub today_open: u32,
```

`task` and `project` keep their meaning: the live task's title and project.
The clock's existing task line works as before.

---

## 4. IPC

### 4.1 Commands (`commands.rs`)

Commands that change tasks emit `chronos://tasks` and, if the live task was
affected, broadcast a snapshot. Both go through a new helper,
`shell::tasks_changed(&app)`.

| Command | Args | Returns | Notes |
|---|---|---|---|
| `tasks_list` | `doneDays: u32` | `TaskRow[]` | All open tasks, plus tasks done in the last `doneDays` local days |
| `task_parse` | `text` | `ParsedTask \| null` | For the preview chips; no write |
| `task_add` | `text`, `bucket`, `top: bool` | `TaskRow` | Parses, then inserts at the top or bottom of `bucket` |
| `task_update` | `id`, `title?`, `estimateMs?` | `TaskRow` | `title` is re-parsed, so `#tag`/estimate edits work inline. `estimateMs: null` clears the estimate |
| `task_move` | `id`, `bucket`, `beforeId?` | `()` | Drag & drop. `beforeId: null` puts it at the end. Sets `moved_at` when the bucket changes |
| `task_set_done` | `id`, `done: bool` | `()` | For the live task, this does what `DoneTask` does. Reopening puts it back in its bucket at the top |
| `task_delete` | `id` | `()` | Hard delete. The engine keeps the last deleted row in memory for undo |
| `task_undo_delete` | — | `TaskRow \| null` | Re-inserts it with the same id, position and bucket (its old time credit is gone) |
| `plan_mode` | — | `"panel" \| "board"` | |
| `set_plan_mode` | `mode` | `()` | Resizes the panel window (§6) |
| `morning_state` | — | `MorningState` | §7 |
| `morning_commit` | `todayIds: number[]` | `()` | §7 |
| `morning_dismiss` | — | `()` | §7 |

### 4.2 Events

| Event | Payload | Sent when |
|---|---|---|
| `chronos://tasks` | none | any task change; the UI refetches `tasks_list` |
| `chronos://plan-mode` | `"panel" \| "board"` | mode changed (toggle, morning prompt, tab switch) |
| `chronos://morning` | none | the morning prompt should open |

### 4.3 Frontend (`src/api.ts`, `src/types.ts`)

- Add each command and event to `Backend`, `tauriBackend()` **and**
  `mockBackend()`, so `npm run dev` still previews in a browser. The mock
  seeds the same sample tasks as the mockup and accepts `?plan=board` and
  `?morning=1`.
- `types.ts`: `Bucket`, `TaskRow`, `ParsedTask`, `MorningState`, the new
  `Action` variants and the `Snapshot` fields.
- `openPanel(tab)` accepts `"plan"`.

---

## 5. The clock

Small changes only. The layouts stay as they are.

### 5.1 Task line (`parts.tsx` → `TaskLine`)

| State | Large / Medium show |
|---|---|
| Focus, task with estimate | `▸ Prep slides` on the left, `38/60m` on the right. Over the estimate: `68/60m` in the warn colour |
| Focus, task without estimate | `▸ Prep slides` on the left, `38m` on the right |
| Break, task set | `Up next: Prep slides` |
| No task, Today has open tasks | the existing *"Set an intention (T)"* hint |
| No task, Today is empty | `Today's list is clear · T to add` |
| Just finished a task | for 4 s: `✓ Prep slides · 42m` (then the next task) |

- `#project` moves from the line into the tooltip, to make room for
  `38/60m` at 240px. The tooltip becomes *"Prep slides · #course · 38 of 60
  min · Next: Tailor resume"*.
- Small and Pill are unchanged. They show the task only in the tooltip, which
  fits their "minimum distraction" purpose.

### 5.2 Keys (`Controls`)

| Situation | Left key | Middle | Right |
|---|---|---|---|
| Focus with a live task | **✓ Done** (D) | Play/Pause | Skip to break (S) |
| Anything else | Reset (R), as now | Play/Pause | Skip, as now |

- **Skip keeps its current meaning** (skip the phase). Your muscle memory
  stays intact; this is a change from the mockup, where Skip meant "next
  task".
- **"Next task (N)"** and **"Reset (R)"** are always in the ⋯ clock menu, and
  the keyboard shortcuts R / ⇧R keep working.
- New keys in `useKeyboard` (`Clock.tsx`): `KeyD` → `doneTask`,
  `KeyN` → `nextTask`.
- The ⋯ clock menu and the tray menu gain: **Done with Task**, **Next Task**,
  **Plan…**, and **Add Task… ⌃⌥T**.

---

## 6. Plan tab, board view and the toggle

### 6.1 One window, two layouts

The Plan tab is a new tab in the existing **panel** window. Board view is the
**same window, widened**; there is no second window.

| | Panel layout | Board layout |
|---|---|---|
| Window size | 480 × 680 (current panel) | 1240 × 760 by default |
| Minimum | 420 × 520 | 960 × 560 |
| Columns | one at a time: segmented **Today · This week · Backlog · Done** | four side by side: **Backlog · This week · Today · Done** |
| Extras | the Today header with planned vs goal | sidebar: project filter, Today's load |

Tabs become **Today · Plan · History · Insights · Settings**. To fit five tabs
in 480px, tab padding drops from `7px 14px` to `7px 10px`.

### 6.2 The toggle

- **Controls:** a ⤢ button at the end of the column switcher (panel layout),
  a **⤡ Panel view** button in the header (board layout), and **⌘E** anywhere
  in the panel window while on Plan.
- **Resizing:** `shell::set_plan_mode` resizes the panel window. The
  "keep the edge nearest the screen side fixed" logic is moved out of
  `size_clock` into a shared `resize_keeping_edge(window, target)`, so a
  panel near the right edge grows to the left. The result is clamped to the
  monitor's visible area.
- **Animation:** none in Phase 1 (an instant resize). A smooth macOS
  `setFrame:display:animate:` through `sysmac.rs` is an optional extra.
- **Remembered:**
  - `kv.plan_mode`: the last mode. The Plan tab reopens in it.
  - `kv.board_size` / `kv.panel_size`: the size you last dragged each layout
    to. The window-state plugin stores only position, as it does now.
- **Other tabs** *(agreed)*: switching to Today, History, Insights or
  Settings while in board view shrinks the window to panel size. Returning to
  Plan widens it again, because Plan remembers `board`.
- **State is kept:** the task you were on, a half-typed quick add and the
  live task stay as they are, because nothing is re-mounted.

### 6.3 Plan tab contents

**Quick add (top of both layouts)**

- One input. As you type, chips preview the parsed result (via `task_parse`):
  `28m · #job-search · → Today`.
- **Enter** adds the task. **Tab** cycles the destination
  Today → This week → Backlog. **Esc** clears.
- In the panel layout, the destination defaults to the column you're viewing
  (Done → Today). In the board layout it defaults to Today.

**Today header (panel layout; "Today's load" in the board sidebar)**

```
projected = focus already done today + Σ max(estimate − spent, 0) over open Today tasks
```

It shows, for example, *"Planned 3h 35m · 1h 10m done + 2h 25m to go ·
35m over your 3h goal"*, with a bar in the focus colour (warn colour when
over). Tasks without an estimate are listed separately (*"+ 2 without an
estimate"*), not counted as zero.

**Task card**

- Checkbox (done / reopen), title, `#project` with its colour dot, estimate
  (or `38/60m` once time is spent), and a *"from Fri"* label for leftovers.
- The live task has an accent border, a LIVE tag and a progress bar.
- Hover actions:
  - **Start** (Today, not live) → `StartTask`
  - **→ Today** (This week)
  - **→ This week** (Backlog)
  - **⋯** menu: Edit, Move to…, Delete
- **Editing:** click the title to edit it inline; Enter saves through
  `task_update` (re-parsed, so typing `45m` changes the estimate).
- **Delete:** a toast appears, *"Deleted · Undo"*, for 6 s
  (`task_undo_delete`).

**Done column**

- Tasks completed today, newest first.
- A collapsed **"Earlier this week"** group lists the previous 6 days.
- Older tasks are only in History.

**Project colours**

Each project gets a stable colour from a fixed palette by hashing its name,
so `#job-search` is the same colour everywhere. No settings needed.

### 6.4 Reordering

- **Pointer-based drag**, written by hand: `pointerdown` / `pointermove` /
  `pointerup` with a placeholder gap. HTML5 drag-and-drop is avoided because
  Tauri's file-drop handling interferes with it.
- In the panel layout, you drag within the column, or onto a column tab to
  move a task there.
- In the board layout, you drag within and across columns.
- **Keyboard** (the card must have focus): **⌥↑/⌥↓** moves it in its column,
  **⌥←/⌥→** moves it to the neighbouring bucket, **Space** marks it done,
  **Enter** edits it, **⌫** deletes it.
- Every drop is a single `task_move(id, bucket, beforeId)`.

### 6.5 Files

```
src/panel/Plan.tsx          tab container: mode, quick add, data loading
src/panel/PlanColumns.tsx   panel layout (segmented) + board layout (4 columns)
src/panel/TaskCard.tsx      one card, shared by both layouts
src/panel/useDragList.ts    pointer drag & drop + keyboard moves
src/panel/Morning.tsx       morning prompt sheet (§7)
src/panel/plan.css          uses the existing panel.css tokens
src/quick/AddTask.tsx       ⌃⌥T popup (§8)
```

---

## 7. Morning prompt *(agreed: opens in board view)*

**When it opens:** the first time in a local day that all of these hold
(checked by the watchdog in `companion.rs`, at most once a minute):

- the time is after `settings.morning_after_hour` (default 5:00),
- you are at the computer (`sysmac::idle_ms() < 60 s`),
- `kv.morning_shown` is not today's date,
- `settings.morning_prompt` is on (default on),
- nothing is being screen-shared (`companion::is_sharing`).

Then Chronos opens the panel on Plan **in board view**, shows the sheet, and
saves today's date in `kv.morning_shown`. Launching the app or waking the Mac
in the morning therefore shows it once, and never again that day.

**The sheet** (a modal over the board, the mockup's "Good morning" card):

| Group | Contents | Default |
|---|---|---|
| Left over | open Today tasks from earlier days | ticked |
| From this week | open Week tasks | not ticked |
| From the backlog | top 5 Backlog tasks (collapsed) | not ticked |

- A running total of ticked estimates vs the daily goal, with the same
  colours as the Today header.
- You can also add a new task from the sheet; it goes to Today, ticked.

**Buttons**

- **Start the day** → `morning_commit(ids)`. Ticked tasks go to Today (in the
  order shown). Unticked leftovers go back to the **top of This week**, so
  Today only holds what you chose. The window then **shrinks back to panel
  view**, and the top Today task is set up on the clock, ready but **not
  started**.
- **Not today** → `morning_dismiss()`. Nothing moves, and the window stays
  in board view.

**Settings** (Settings tab, new "Planning" card):

- "Ask me to plan my day each morning" (on/off)
- "Not before" (hour picker)
- "Quick add shortcut" (§8)

---

## 8. Quick add from any app (⌃⌥T)

- **Shortcut:** a new setting, `quick_add_shortcut`, default
  `"Control+Alt+T"`.
  - `shell::register_shortcuts` registers **three** shortcuts. Its error and
    rollback handling covers all three, like the existing two.
  - `set_settings` re-registers when any of the three changes.
- **Popup:** reuses the `quick` window with a new variant,
  `Quick::AddTask`. It differs from the distraction and reflection popups:

| | Distraction / Reflection | Add Task |
|---|---|---|
| Size | 330 × 146 / 186 | **560 × 132** |
| Position | next to the clock | **centred, upper third of the active screen** |
| Focus | distraction only | **always focused** (you type straight away) |
| Auto-close | yes | **no**; Esc or Enter closes it |

- **Content:** the same input and chips as the Plan tab's quick add.
  - Enter adds to Today (Tab changes the destination).
  - ⌘Enter adds the task **and starts it** (`StartTask`).
- **When the clock is hidden** (⌘⇧H or auto-hide): the popup still works,
  since capture shouldn't depend on the clock being visible.

---

## 9. Settings additions (`settings.rs`)

```rust
pub quick_add_shortcut: String,  // "Control+Alt+T"
pub morning_prompt: bool,        // true
pub morning_after_hour: u32,     // 5, clamped 0–12
```

All are covered by `#[serde(default)]`; `sanitized()` clamps the hour.

KV keys: `plan_mode`, `panel_size`, `board_size`, `morning_shown`.

---

## 10. Edge cases

| Case | Behaviour |
|---|---|
| App crash mid-block with a live task | `timer_state` holds `task_id`/`task_ms`/`task_since_ms`; on relaunch the restored block completes and credits correctly (same path as `state_survives_restart`) |
| Live task edited in the panel (title/estimate) | Snapshot broadcast; clock updates immediately; session text takes the new title at finish |
| Live task deleted | Its stretch is discarded, next Today task goes live, block continues |
| Live task dragged to This week | Stretch credited, next Today task goes live |
| Done in a break (from the panel) | Marked done; *Up next* moves to the next task |
| Midnight during a block | Block belongs to its start day (unchanged); task credit goes to the task as usual; leftovers are judged from `moved_at` |
| Same title typed twice | Two tasks. Only `SetTask` from the clock matches by title, and only among open Today tasks |
| 300+ tasks | `tasks_list` is one indexed query; spent time via one `GROUP BY task_id` join, no N+1 |
| Two windows editing at once | Each change is a command; every window refetches on `chronos://tasks`; last write wins |

---

## 11. Tests

**Rust** (`npm run test:rust`), in the style of the current `engine.rs` tests:

- `tasks::parse_*`: every row in §2, plus the 8 h limit and a 120-char title.
- `db::migration_3_*`: migrations stay idempotent; `ON DELETE` keeps
  sessions; `position` renumbering.
- `engine::live_task_*`:
  - the top Today task goes live on start;
  - reordering doesn't switch it;
  - `StartTask` from Backlog moves it to the top of Today;
  - `StartTask` during a break switches to focus.
- `engine::task_credit_*`:
  - a 25 min block with a switch at minute 10 gives (10, 15);
  - pause and away time are not credited;
  - a cancelled block credits nothing;
  - an abandoned block keeps its credit;
  - `sessions.task` is the task with the most time.
- `engine::done_task_keeps_timer_running` and `next_task_moves_to_bottom`.
- `engine::task_state_survives_restart`.
- `engine::morning_*`:
  - shown once per day;
  - not before the hour, not while sharing;
  - commit moves ticked tasks and returns unticked leftovers to This week.
- `shell::register_three_shortcuts_rolls_back`, extending the existing
  pattern.

**Frontend** (`npm test`):

- `useDragList` maths: target index and `beforeId` for drops at the top,
  middle and end.
- Projected-load formula, including tasks without an estimate.

**By hand** (`npm run tauri dev`), before calling Phase 1 done:

- [ ] ⌃⌥T from Safari, type `Prep slides 28min #job-search`, Enter → appears in Today.
- [ ] Start focus → the top task goes live; the clock shows `0/28m`, rising.
- [ ] Press D mid-block → `✓` flash, the next task goes live, the timer doesn't stop.
- [ ] ⌘E in Plan → widens to the board (grows left if near the right edge); ⌘E again → shrinks back.
- [ ] Board view → click History → shrinks; back to Plan → widens.
- [ ] Set the clock forward to 06:00 the next day → the morning sheet opens in board view; Start the day → shrinks to the panel, top task ready.
- [ ] Quit mid-block, relaunch → task time is correct.
- [ ] History, Insights, the weekly score and the email still show the same numbers for old data.

---

## 12. Build order

Each step ends with passing tests and a working app. Commit after each.

| # | Step | Main files | Size |
|---|---|---|---|
| 1 | Migration 3, `tasks.rs` parsing, DB queries | `db.rs`, `tasks.rs` | M |
| 2 | Live task, crediting, new actions, snapshot fields | `timer.rs`, `engine.rs` | L |
| 3 | Commands, events, `api.ts` + mock, `types.ts` | `commands.rs`, `lib.rs`, `shell.rs`, `src/api.ts` | M |
| 4 | Clock: task line, Done key, D/N keys, menu items | `parts.tsx`, `Clock.tsx`, `shell.rs` | S |
| 4b | Themes: `theme` setting (Glass default, Retro), frosted panel window, Glass tokens | `settings.rs`, `shell.rs`, `panel.css`, `hooks.ts` | M |
| 5 | Plan tab (panel layout), quick add, cards, drag | `src/panel/*` | L |
| 6 | Board layout + toggle + window resizing | `PlanColumns.tsx`, `Panel.tsx`, `shell.rs` | M |
| 7 | ⌃⌥T popup | `companion.rs`, `src/quick/AddTask.tsx`, `settings.rs` | M |
| 8 | Morning prompt + Planning settings card | `companion.rs`, `Morning.tsx`, `SettingsView.tsx` | M |
| 9 | Glass clock style, and Glass versions of the clock's pop-ups | `src/clock/*`, `src/quick/*`, `settings.rs` | M |
| 10 | Docs: README features/shortcuts, `DESIGN.md` schema section | docs | S |

---

## 13. Changes from the mockup

- **Skip on the clock still means "skip to break".** "Next task" is the N
  key and a menu item, so existing habits don't break.
- **A task ticked off without a timer has no tracked time.** It shows
  *"no time tracked"*, not "on time". Logging time by hand comes in Phase 2.
- **`#project` moves into the clock's tooltip** to make room for `38/60m`.

---

## 14. Themes *(agreed 2026-10-13)*

The orange-heavy look was overwhelming on the larger planner screens, so the
panel, board and their pop-ups get a second theme. Mockups: the "Glass" row of
the planner canvas.

| | Glass (default) | Retro |
|---|---|---|
| Where | panel tabs, board, ⌃⌥T popup, morning sheet | the same, chosen in Settings → Appearance |
| Window | frosted macOS window (`Effect::Sidebar` vibrancy, overlay title bar with the traffic lights inside the header) | the same window, painted opaque in cream |
| Colour | neutral greys and white; orange (`#FF9500`, dark `#FF9F0A`) only for the live task, progress, the one main button and ticked checks | today's cream and orange |
| Orange text | `#C93400` light / `#FFB340` dark (readable); buttons put dark text on orange | as today |
| Type | system font (SF Pro), 13px base | Avenir Next, as today |
| Light/dark | follows macOS | follows macOS |

- `settings.theme`: `"glass" | "retro"`, default `"glass"`, so existing
  installs switch to Glass and can switch back.
- The clock keeps its own `style` (Retro default, Minimal); **Glass becomes a
  third clock style** in step 9. The clock's distraction and reflection
  pop-ups follow the clock's style, not the panel theme.
- macOS 13 has no Liquid Glass (macOS 26 only); the system vibrancy blur is
  what shows through. The browser preview fakes it with `backdrop-filter`
  over a desktop-like backdrop.
