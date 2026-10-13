# Chronos: Reminders and Calendar

Chronos reads your Reminders and your calendars through macOS itself
(EventKit), the same way Fantastical or Things do. Any account you have added
to the Mac's Reminders or Calendar app works: iCloud, Google, Outlook. There
is no Google sign-in in Chronos, and nothing leaves your Mac.

Decisions (agreed 2026-10-13):

- **Reminders:** import plus completion both ways. Reminders you pick become
  Chronos tasks; ticking one off on either side ticks it off on the other.
  Tasks you create in Chronos stay in Chronos.
- **Which reminders:** only the lists you tick in Settings.
- **Calendar:** read-only, through the Mac's Calendar app. Today's meetings
  show in Chronos and count against the time left in your day.

---

## 1. Access

- `src-tauri/Info.plist` adds the usage strings macOS shows in its prompt:
  `NSRemindersUsageDescription`, `NSRemindersFullAccessUsageDescription`,
  `NSCalendarsUsageDescription`, `NSCalendarsFullAccessUsageDescription`.
- Access is asked for only when you press **Connect** in Settings, never at
  launch. macOS 14+ uses `requestFullAccessTo…`; macOS 13 uses
  `requestAccessToEntityType:`.
- Denied or restricted: Settings says so and offers **Open Privacy
  Settings**. Nothing else in Chronos changes.

`src-tauri/src/eventkit.rs` (new) is the only file that talks to EventKit.
It exposes plain Rust types so the sync logic can be tested without a Mac:

```rust
pub struct ListInfo { pub id: String, pub title: String, pub color: String, pub source: String }
pub struct Reminder {
    pub id: String,          // calendarItemIdentifier
    pub list_id: String,
    pub title: String,
    pub due: Option<i64>,    // ms; date-only dues are local midnight
    pub completed_at: Option<i64>,
    pub modified_at: i64,
}
pub struct Event { pub id: String, pub calendar_id: String, pub title: String, pub start: i64, pub end: i64, pub all_day: bool }
```

## 2. Reminders

### 2.1 Storage (migration 4)

```sql
ALTER TABLE tasks ADD COLUMN ext_id    TEXT;    -- reminder id; NULL = a Chronos task
ALTER TABLE tasks ADD COLUMN ext_list  TEXT;    -- its list id
ALTER TABLE tasks ADD COLUMN ext_title TEXT;    -- title as last seen in Reminders
ALTER TABLE tasks ADD COLUMN ext_due   INTEGER; -- due as last seen in Reminders
CREATE UNIQUE INDEX idx_tasks_ext ON tasks(ext_id) WHERE ext_id IS NOT NULL;
```

Settings: `reminder_lists: [{ id, project }]`, the ticked lists, each with the
`#project` its tasks get (default: the list name as a tag; empty for none).

### 2.2 Sync rules (`src-tauri/src/remsync.rs`, pure, unit-tested)

| In Reminders | In Chronos |
|---|---|
| New open reminder in a ticked list | New task. Title parsed like quick add (`Call mom 15m` gets a 15m estimate; a `#tag` beats the list's project). Bucket from the due date: overdue or today → Today, later this week → This week, later or none → Backlog. Added at the end of the bucket. |
| Title changed | Title follows, unless it is the same as `ext_title` (so a rename in Chronos stays until Reminders changes it). |
| Due date changed to today or earlier | Moves to Today if it isn't there. Other due changes don't move it. |
| Completed | Task ticked off, `done_at` = completion time. |
| Uncompleted | Task reopened at the end of its bucket. |
| Deleted, or moved to an unticked list | Task removed if it has no focus time; otherwise unlinked and kept as a Chronos task. |

| In Chronos | In Reminders |
|---|---|
| Imported task ticked off | Reminder completed |
| Imported task reopened | Reminder uncompleted |
| Renamed, moved, re-estimated, deleted | Nothing (Chronos-only). Deleting unlinks; it won't be imported again unless it changes in Reminders. |
| List unticked in Settings | Its open tasks with no focus time are removed; the rest are unlinked. |

### 2.3 When it syncs

On launch, every 2 minutes, when the panel opens, right after a tick in
Chronos (that write is immediate), and on `EKEventStoreChangedNotification`.
EventKit work runs on a background thread; the engine applies the result in
one transaction and emits `chronos://tasks`.

### 2.4 In the UI

Imported tasks show a small Reminders mark next to the project. Everything
else about them works like any task.

## 3. Calendar

- Settings: `calendars: [id]`, the ticked calendars; `workday_end_hour`
  (default 18).
- Today's events are read when the panel opens and every 5 minutes.
  All-day events show as a chip and don't count.
- **Today tab:** meetings are drawn on the timeline as their own blocks and
  listed in the sessions card's time order.
- **Plan (panel and board):** a "Meetings" strip above the Today list:
  `10:00 Standup · 2:30 Design review +1`, expandable.
- **Load:** the load line adds meetings still to come and checks the day:
  `3h 49m planned · 1h 30m in meetings · doesn't fit before 6 PM by 40m`.
  The goal bar itself is unchanged; it is still about focus time.

## 4. Settings

A sixth section, **Connect**, with two groups:

- **Reminders:** Connect / Connected ✓ · the lists with a tick and a project
  each · Sync now · last synced.
- **Calendar:** Connect / Connected ✓ · the calendars with a tick · Workday
  ends at.

## 5. Build order

1. EventKit bridge, Info.plist, the Connect section with permission and your
   lists and calendars. Check on your Mac that the prompt appears and lists
   load.
2. Migration 4, the sync rules with tests, the background sync and the
   write-back on tick.
3. Reminders mark on cards, Sync now, error states.
4. Calendar: events, the meetings strip, the timeline blocks and the load
   line.
5. Docs, README, preview mocks.
