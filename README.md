# Chronos

A small retro-futuristic Pomodoro clock that floats on your desktop. It helps
you focus, hides when you need it to, and keeps a private local record of how
you work.

Built with Tauri 2 (Rust) and React. Design notes are in
[`docs/DESIGN.md`](docs/DESIGN.md).

## Run it

Prerequisites: Node 20+, Rust (`rustup`) and the Xcode Command Line Tools.

```bash
npm install
npm run tauri dev                    # development, hot reload
npm run tauri build -- --bundles app # → src-tauri/target/release/bundle/macos/Chronos.app
```

### Install as an app

```bash
npm run tauri build -- --bundles app
cp -R src-tauri/target/release/bundle/macos/Chronos.app /Applications/
open -a Chronos
```

Chronos then shows up in Launchpad and Spotlight. **Open at login** is on by
default (Settings → Startup), so it comes back by itself after a restart.
Opening the app while it's already running brings a hidden clock back.

Chronos lives in the menu bar (look for the little alarm clock). To give it a
Dock icon as well, turn on Settings → Startup → Show in Dock.

## Widget sizes

| Widget | What it shows |
|---|---|
| **Pill** | Time, a progress line and one key. The least distracting option. |
| **Small** | The dial alone. Keys appear on hover and whenever the timer is idle. |
| **Medium** | The dial plus phase, session, task and keys |
| **Large** | The full desk clock |

Each comes in **Retro** or **Minimal** style. Minimal follows the system's
light or dark appearance. Switch from the ⋯ menu → Widget Size, or in
Settings, which also has a size slider (60–200%).

## Using the clock

| Do this | To |
|---|---|
| ▶ / Space | Start, pause or resume |
| ↺ / R (⇧R) | Reset (restart) the current session |
| ⏭ / S | Skip to the next phase |
| Hover the dial → −5 · +5 · +10 | Change time without restarting |
| Scroll on the dial / ↑ ↓ (⇧ = 5 min) | Nudge time ±1 min |
| Click the digits / Enter | Type a time: `25`, `18:42`, `1h 15m` |
| Click the line under the dial / T | Set what you're working on |
| ⋯ or right-click | Menu: switch phase, focus modes, widget size & style, always on top, stats, settings |
| ⌘⇧H (configurable), the eye button or the menu bar | Hide or show the clock. **The timer keeps running.** |
| Drag anywhere / bottom-right corner | Move / resize (position and size are remembered) |

## What's in the MVP (Phase 1)

- Floating, transparent, draggable and resizable clock, always on top, with adjustable opacity
- Focus, short break and long break (every N sessions), plus presets: Standard 25/5, Deep Work 50/10, Quick 15/5 and Custom
- Start, pause, reset, restart and skip; live +/− time, setting an exact time, switching phase
- Hide mode via a global shortcut, the tray or a button. Optional invisibility in screen captures.
- Timer based on absolute end times, run by a Rust watchdog. It survives sleep, hidden windows and app restarts.
- Every session recorded in local SQLite. A Today panel shows sessions, completed, interrupted and abandoned counts, focused and break time, average and longest focus, streak, a timeline and a log.
- Completion chime with volume control, a daily focus goal bar, and an optional task intention

Not yet built (Phases 2–3 in the brief): weekly and monthly history, distraction
logging (⌘D), end-of-session reflection, notifications, productivity score,
insights and adaptive suggestions, and themes. The schema already has the
`distractions` table and `rating` / `goal_completed` columns those need.

## Tests

```bash
npm test           # frontend: formatting, parsing, shortcut capture
npm run test:rust  # timer state machine, database, engine lifecycle, crash recovery
```

## Privacy

All data lives in `~/Library/Application Support/com.chronos.desk/chronos.sqlite3`.
There is no account, no telemetry and no network access.
