# Chronos and Claude Code

When Claude Code needs you to do something yourself (log in, add a key,
approve something, test on a device), it puts a to-do at the top of Today in
Chronos (or This week or the Backlog, if you ask), tagged with the project
it's working in. It works in every project, not just one.

Agreed 2026-10-13:

- Claude's to-dos go to the **top of Today**, tagged `#<project folder>`,
  unless you ask for **This week** or the **Backlog** (added 2026-10-14).
- Claude can **add, list, move and tick off its own** to-dos; it never sees or
  changes the tasks you write.
- Each to-do has a short title and a **note** with the details (the exact
  command, file or link).
- Only **real action items** outside the chat, not ordinary questions.

## How it works

`Chronos.app/Contents/MacOS/chronos mcp` is a small MCP server on
stdin/stdout, built into the app (`src-tauri/src/mcp.rs`). Claude Code starts
it when needed. It opens the same database as the app, so it works even when
Chronos is closed, and a running Chronos shows new to-dos within about two
seconds (it watches SQLite's `data_version`).

| Tool | What it does |
|---|---|
| `add_todo(title, note?, project?, estimate_minutes?, list?)` | Adds to the top of `list` (`today`, the default, `this_week` or `backlog`) with `source = "claude"`. The project defaults to the folder Claude Code is working in. If the same open to-do exists, returns it and the list it's in instead of adding a second. |
| `list_todos(include_done?)` | Claude's open to-dos and the list each is in (and, with `include_done`, those finished in the last 7 days). |
| `move_todo(id, list)` | Moves one to the top of Today, This week or the Backlog. Refuses tasks Claude didn't add. |
| `complete_todo(id)` | Ticks one off. Refuses tasks Claude didn't add. |

In the Plan tab and on the board these tasks show a **Claude** badge, and
their note under the title (hover for the whole note). Otherwise they're
ordinary tasks: drag, edit, start or delete them as usual.

Storage: migration 4 adds `tasks.note` and `tasks.source` (NULL = you).

## Setting it up (once, for all projects)

1. Register the server for your user, so every project has it:

   ```sh
   claude mcp add --scope user chronos -- /Applications/Chronos.app/Contents/MacOS/chronos mcp
   ```

2. Tell Claude when to use it, in `~/.claude/CLAUDE.md` (read in every
   project):

   ```markdown
   ## Chronos to-dos

   When you need me to do something myself outside this chat (log in, add a
   key or secret, pay, approve, test on a device, reply to someone), add it to
   Chronos with the `chronos` MCP tool `add_todo`: a short imperative title,
   and a note with the exact command, file or link. Check `list_todos` first so
   you don't add the same thing twice, and tick it off with `complete_todo`
   once you can see it's done. Don't add ordinary questions you're asking me
   in the chat.
   ```

To check it: `claude mcp list` shows `chronos: … ✔ Connected`.

To turn it off: `claude mcp remove --scope user chronos`, and delete that
section from `~/.claude/CLAUDE.md`.
