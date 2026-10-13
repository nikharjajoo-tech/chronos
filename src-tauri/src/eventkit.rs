//! The only file that talks to EventKit, the system service behind the
//! Reminders and Calendar apps. Every call runs on one background thread that
//! owns the event store, so EventKit objects never cross threads and a slow
//! permission prompt never blocks the UI. Callers get plain Rust types back.
//! Off macOS everything reports `Unavailable`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    NotDetermined,
    Denied,
    Restricted,
    Granted,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Reminders,
    Calendar,
}

/// A Reminders list or a calendar.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListInfo {
    pub id: String,
    pub title: String,
    /// The account it belongs to: "iCloud", "Google", "On My Mac", …
    pub source: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reminder {
    pub id: String,
    pub list_id: String,
    pub title: String,
    /// Local time in ms; a date-only due is that day's midnight.
    pub due: Option<i64>,
    pub completed_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    /// Unique per occurrence, so a repeating meeting shows once per day.
    pub id: String,
    pub calendar_id: String,
    pub title: String,
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
}

pub use imp::*;

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use block2::RcBlock;
    use objc2::rc::{autoreleasepool, Retained};
    use objc2::runtime::{Bool, NSObjectProtocol};
    use objc2::sel;
    use objc2_event_kit::{EKAuthorizationStatus, EKCalendar, EKEntityType, EKEventStore, EKReminder};
    use objc2_foundation::{NSArray, NSCalendar, NSDate, NSError, NSPredicate, NSString};
    use std::sync::{mpsc, Mutex, OnceLock};
    use std::time::Duration;

    type Job = Box<dyn FnOnce(&mut Retained<EKEventStore>) + Send>;
    static WORKER: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
    const GONE: &str = "Couldn't reach Reminders and Calendar.";

    /// Run `f` on the EventKit thread and wait for its result.
    fn run<T: Send + 'static>(f: impl FnOnce(&mut Retained<EKEventStore>) -> T + Send + 'static) -> Result<T, String> {
        let tx = WORKER.get_or_init(|| {
            let (tx, rx) = mpsc::channel::<Job>();
            std::thread::Builder::new()
                .name("eventkit".into())
                .spawn(move || {
                    let mut store = unsafe { EKEventStore::new() };
                    for job in rx {
                        autoreleasepool(|_| job(&mut store));
                    }
                })
                .expect("start the EventKit thread");
            Mutex::new(tx)
        });
        let (rtx, rrx) = mpsc::channel();
        let job: Job = Box::new(move |store| {
            let _ = rtx.send(f(store));
        });
        tx.lock().unwrap_or_else(|p| p.into_inner()).send(job).map_err(|_| GONE.to_string())?;
        rrx.recv().map_err(|_| GONE.to_string())
    }

    fn entity(kind: Kind) -> EKEntityType {
        match kind {
            Kind::Reminders => EKEntityType::Reminder,
            Kind::Calendar => EKEntityType::Event,
        }
    }

    fn ms(d: &NSDate) -> i64 {
        (d.timeIntervalSince1970() * 1000.0) as i64
    }

    fn date(ms: i64) -> Retained<NSDate> {
        NSDate::dateWithTimeIntervalSince1970(ms as f64 / 1000.0)
    }

    fn message(e: &NSError) -> String {
        e.localizedDescription().to_string()
    }

    pub fn access(kind: Kind) -> Access {
        let status = unsafe { EKEventStore::authorizationStatusForEntityType(entity(kind)) };
        match status {
            EKAuthorizationStatus::NotDetermined => Access::NotDetermined,
            EKAuthorizationStatus::Restricted => Access::Restricted,
            EKAuthorizationStatus::FullAccess => Access::Granted,
            // Write-only calendar access can't read anything, which is all we do.
            _ => Access::Denied,
        }
    }

    /// Shows macOS's prompt the first time; afterwards answers at once with
    /// what the user chose. Blocks until they answer.
    pub fn request_access(kind: Kind) -> Result<Access, String> {
        run(move |store| {
            let (tx, rx) = mpsc::channel::<bool>();
            let done = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
                let _ = tx.send(granted.as_bool());
            });
            let handler = RcBlock::as_ptr(&done);
            // macOS 14 split access into "full" and "write-only"; 13 has one kind.
            let full = match kind {
                Kind::Reminders => sel!(requestFullAccessToRemindersWithCompletion:),
                Kind::Calendar => sel!(requestFullAccessToEventsWithCompletion:),
            };
            unsafe {
                if store.respondsToSelector(full) {
                    match kind {
                        Kind::Reminders => store.requestFullAccessToRemindersWithCompletion(handler),
                        Kind::Calendar => store.requestFullAccessToEventsWithCompletion(handler),
                    }
                } else {
                    #[allow(deprecated)]
                    store.requestAccessToEntityType_completion(entity(kind), handler);
                }
            }
            if rx.recv_timeout(Duration::from_secs(600)).unwrap_or(false) {
                // A store made before access was granted never sees the data.
                *store = unsafe { EKEventStore::new() };
            }
            access(kind)
        })
    }

    fn all_lists(store: &EKEventStore, kind: Kind) -> Vec<Retained<EKCalendar>> {
        let cals = unsafe { store.calendarsForEntityType(entity(kind)) };
        (0..cals.count()).map(|i| cals.objectAtIndex(i)).collect()
    }

    fn lists_with_ids(store: &EKEventStore, kind: Kind, ids: &[String]) -> Retained<NSArray<EKCalendar>> {
        let picked: Vec<_> = all_lists(store, kind)
            .into_iter()
            .filter(|c| ids.contains(&unsafe { c.calendarIdentifier() }.to_string()))
            .collect();
        NSArray::from_retained_slice(&picked)
    }

    pub fn lists(kind: Kind) -> Result<Vec<ListInfo>, String> {
        run(move |store| {
            all_lists(store, kind)
                .iter()
                .map(|c| unsafe {
                    ListInfo {
                        id: c.calendarIdentifier().to_string(),
                        title: c.title().to_string(),
                        source: c.source().map(|s| s.title().to_string()).unwrap_or_default(),
                    }
                })
                .collect()
        })
    }

    fn reminder(r: &EKReminder, now: i64) -> Option<Reminder> {
        unsafe {
            let list_id = r.calendar()?.calendarIdentifier().to_string();
            let due = r.dueDateComponents().and_then(|c| NSCalendar::currentCalendar().dateFromComponents(&c)).map(|d| ms(&d));
            let completed_at = r.isCompleted().then(|| r.completionDate().map(|d| ms(&d)).unwrap_or(now));
            Some(Reminder { id: r.calendarItemIdentifier().to_string(), list_id, title: r.title().to_string(), due, completed_at })
        }
    }

    fn fetch(store: &EKEventStore, predicate: &NSPredicate, now: i64) -> Result<Vec<Reminder>, String> {
        let (tx, rx) = mpsc::channel::<Vec<Reminder>>();
        let done = RcBlock::new(move |found: *mut NSArray<EKReminder>| {
            let rows = match unsafe { found.as_ref() } {
                Some(arr) => (0..arr.count()).filter_map(|i| reminder(&arr.objectAtIndex(i), now)).collect(),
                None => Vec::new(),
            };
            let _ = tx.send(rows);
        });
        unsafe { store.fetchRemindersMatchingPredicate_completion(predicate, &done) };
        rx.recv_timeout(Duration::from_secs(30)).map_err(|_| "Reminders didn't answer in time.".to_string())
    }

    /// Open reminders in the given lists, plus those completed since `completed_since`.
    pub fn reminders(list_ids: Vec<String>, completed_since: i64, now: i64) -> Result<Vec<Reminder>, String> {
        run(move |store| {
            let lists = lists_with_ids(store, Kind::Reminders, &list_ids);
            if lists.count() == 0 {
                return Ok(Vec::new());
            }
            let open = unsafe { store.predicateForIncompleteRemindersWithDueDateStarting_ending_calendars(None, None, Some(&lists)) };
            let since = date(completed_since);
            let done = unsafe { store.predicateForCompletedRemindersWithCompletionDateStarting_ending_calendars(Some(&since), None, Some(&lists)) };
            let mut rows = fetch(store, &open, now)?;
            rows.extend(fetch(store, &done, now)?);
            Ok(rows)
        })?
    }

    /// Tick a reminder off, or reopen it.
    pub fn set_completed(id: String, completed: bool) -> Result<(), String> {
        run(move |store| unsafe {
            let item = store.calendarItemWithIdentifier(&NSString::from_str(&id)).ok_or("That reminder no longer exists.")?;
            let reminder = item.downcast::<EKReminder>().map_err(|_| "That isn't a reminder.".to_string())?;
            reminder.setCompleted(completed);
            store.saveReminder_commit_error(&reminder, true).map_err(|e| message(&e))
        })?
    }

    /// Events overlapping `from..to` in the given calendars, earliest first.
    pub fn events(calendar_ids: Vec<String>, from: i64, to: i64) -> Result<Vec<Event>, String> {
        run(move |store| unsafe {
            let cals = lists_with_ids(store, Kind::Calendar, &calendar_ids);
            if cals.count() == 0 {
                return Vec::new();
            }
            let predicate = store.predicateForEventsWithStartDate_endDate_calendars(&date(from), &date(to), Some(&cals));
            let found = store.eventsMatchingPredicate(&predicate);
            let mut events: Vec<Event> = (0..found.count())
                .filter_map(|i| {
                    let e = found.objectAtIndex(i);
                    let start = ms(&e.startDate());
                    let calendar_id = e.calendar()?.calendarIdentifier().to_string();
                    let id = e.eventIdentifier().map(|s| s.to_string()).unwrap_or_default();
                    Some(Event { id: format!("{id}@{start}"), calendar_id, title: e.title().to_string(), start, end: ms(&e.endDate()), all_day: e.isAllDay() })
                })
                .collect();
            events.sort_by_key(|e| (e.start, e.end));
            events
        })
    }

    /// The Privacy pane where access can be turned back on.
    pub fn open_privacy(kind: Kind) {
        let pane = match kind {
            Kind::Reminders => "Privacy_Reminders",
            Kind::Calendar => "Privacy_Calendars",
        };
        let _ = std::process::Command::new("/usr/bin/open")
            .arg(format!("x-apple.systempreferences:com.apple.preference.security?{pane}"))
            .spawn();
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::*;
    const NONE: &str = "Reminders and Calendar are only available on macOS.";

    pub fn access(_: Kind) -> Access {
        Access::Unavailable
    }
    pub fn request_access(_: Kind) -> Result<Access, String> {
        Ok(Access::Unavailable)
    }
    pub fn lists(_: Kind) -> Result<Vec<ListInfo>, String> {
        Err(NONE.into())
    }
    pub fn reminders(_: Vec<String>, _: i64, _: i64) -> Result<Vec<Reminder>, String> {
        Err(NONE.into())
    }
    pub fn set_completed(_: String, _: bool) -> Result<(), String> {
        Err(NONE.into())
    }
    pub fn events(_: Vec<String>, _: i64, _: i64) -> Result<Vec<Event>, String> {
        Err(NONE.into())
    }
    pub fn open_privacy(_: Kind) {}
}
