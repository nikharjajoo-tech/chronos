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

## Features

**Timer**
- Focus, short break and long break, with presets (Standard 25/5, Deep Work 50/10, Quick 15/5) and custom durations
- Live +/− time, typing an exact time, switching phase, restart and skip
- Wall-clock engine in Rust. It survives sleep, hidden windows and crashes.

**Staying out of the way**
- Hide mode (⌘⇧H, the tray or a button). The timer keeps running.
- **Auto-hide while sharing or recording.** Hides while Zoom screen share, the ⌘⇧5 recorder, OBS, or any app you add is running, and comes back afterwards.
- Optional invisibility in screenshots and recordings
- **Menu-bar countdown:** off, when hidden, or always

**Honest focus data**
- **Away detection.** After N idle minutes, or a closed lid, focus pauses as of when you left, so away time never counts.
- **Do Not Disturb** during focus, through two Shortcuts that Chronos installs for you (Settings → Focus helpers)
- **Distraction log:** ⌃⌥D from any app, with an optional reason (keys 1–7)
- **Reflection** after each focus session (😫 😐 🙂 🔥, plus "did you finish it?"). Always skippable.
- **Projects:** add `#project` to the task, e.g. `Finish resume #job-search`
- Notifications when a session ends or is paused while you're away

**Stats** (Today and History tabs)
- Today: focus vs goal, streak, sessions, completed, interrupted and abandoned, distractions, a timeline, projects and a log
- History by week or month: focus time, average per day, completion rate, average session, distractions, consistency, streak, average rating. Each is compared with the same days of the previous period.
- Per-day bars, best focus hours, distraction peaks, and breakdowns by project and by distraction reason

**Coaching** (Insights tab)
- **Weekly score (0–100)** from focus time vs goal, consistency, finishing sessions, distractions per focus hour, taking breaks, and session ratings. Each part shows its evidence, the change vs the same days last week, and which part moved it most, plus an 8-week trend.
- **Insights** drawn from the last 4 weeks: best start window, where sessions get abandoned, distraction peak and main reason, session-length trend, strongest weekday, and ratings vs length. Each one only appears with enough data, and shows what it's based on.
- **One suggestion at a time** (longer or shorter sessions, Do Not Disturb, auto-start breaks, protect your best hours), with one-click apply. Declined suggestions never come back.
- **Companion lines** on the clock after a session, at most one every 90 minutes: daily goal hit, a long run without a real break, or skipped breaks.

## Tests

```bash
npm test           # frontend: formatting, parsing, shortcut capture
npm run test:rust  # timer, away pauses, database, analytics, score, insights, suggestions, companion, crash recovery
```

## Privacy

All data lives in `~/Library/Application Support/com.chronos.desk/chronos.sqlite3`.
There is no account, no telemetry and no network access.
