//! Local SQLite storage. Schema changes are appended to `MIGRATIONS` and
//! tracked with `PRAGMA user_version`; never edit a migration once shipped.

use crate::timer::{EndStatus, Finished, Phase};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use std::path::Path;

pub type Result<T> = rusqlite::Result<T>;

const MIGRATIONS: &[&str] = &[
    // 1 — initial schema
    r#"
    CREATE TABLE sessions (
      id              INTEGER PRIMARY KEY,
      kind            TEXT    NOT NULL,
      status          TEXT    NOT NULL,
      started_at      INTEGER NOT NULL,
      ended_at        INTEGER,
      planned_ms      INTEGER NOT NULL,
      actual_ms       INTEGER,
      pause_count     INTEGER NOT NULL DEFAULT 0,
      adjust_ms       INTEGER NOT NULL DEFAULT 0,
      task            TEXT,
      preset          TEXT,
      rating          INTEGER,
      goal_completed  TEXT,
      created_at      INTEGER NOT NULL
    );
    CREATE INDEX idx_sessions_started ON sessions(started_at);

    CREATE TABLE distractions (
      id          INTEGER PRIMARY KEY,
      at          INTEGER NOT NULL,
      category    TEXT,
      note        TEXT,
      session_id  INTEGER REFERENCES sessions(id) ON DELETE SET NULL
    );
    CREATE INDEX idx_distractions_at ON distractions(at);

    CREATE TABLE kv (
      key   TEXT PRIMARY KEY,
      value TEXT NOT NULL
    );
    "#,
];

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub id: i64,
    pub kind: String,
    pub status: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub planned_ms: i64,
    pub actual_ms: i64,
    pub pause_count: u32,
    pub task: Option<String>,
}

impl SessionRow {
    fn is_focus(&self) -> bool {
        self.kind == Phase::Focus.as_str()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayStats {
    pub focus_sessions: u32,
    pub completed: u32,
    pub abandoned: u32,
    /// Focus sessions that were paused at least once.
    pub interrupted: u32,
    pub focus_ms: i64,
    pub break_ms: i64,
    pub avg_focus_ms: i64,
    pub longest_focus_ms: i64,
    /// Consecutive completed focus sessions, counting back from the latest.
    pub current_streak: u32,
    pub first_start: Option<i64>,
    pub last_end: Option<i64>,
    /// Finished sessions in the range (cancelled mis-clicks excluded), oldest first.
    pub sessions: Vec<SessionRow>,
}

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        Db::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Db> {
        Db::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Db> {
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        let db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let current: usize = self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
            let tx = self.conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", i + 1)?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn kv_get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT value FROM kv WHERE key = ?1", [key], |r| r.get(0))
            .optional()
            .ok()
            .flatten();
        raw.and_then(|s| serde_json::from_str(&s).ok())
    }

    pub fn kv_set<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let json = serde_json::to_string(value).expect("serializable");
        self.conn.execute(
            "INSERT INTO kv (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, json],
        )?;
        Ok(())
    }

    /// Record the start of a block. Returns the new session id.
    pub fn insert_session(
        &self,
        phase: Phase,
        started_at: i64,
        planned_ms: i64,
        task: Option<&str>,
        preset: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO sessions (kind, status, started_at, planned_ms, task, preset, created_at)
             VALUES (?1, 'running', ?2, ?3, ?4, ?5, ?2)",
            params![phase.as_str(), started_at, planned_ms, task, preset],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn finish_session(&self, f: &Finished, preset: &str) -> Result<i64> {
        let id = match f.session_id {
            Some(id) => id,
            None => self.insert_session(f.phase, f.started_at, f.planned_ms, f.task.as_deref(), preset)?,
        };
        self.conn.execute(
            "UPDATE sessions SET status = ?2, ended_at = ?3, planned_ms = ?4, actual_ms = ?5,
                    pause_count = ?6, adjust_ms = ?7, task = ?8
             WHERE id = ?1",
            params![
                id,
                f.status.as_str(),
                f.ended_at,
                f.planned_ms,
                f.actual_ms,
                f.pause_count,
                f.adjust_ms,
                f.task
            ],
        )?;
        Ok(id)
    }

    pub fn set_session_task(&self, id: i64, task: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE sessions SET task = ?2 WHERE id = ?1", params![id, task])?;
        Ok(())
    }

    /// Sessions left `running` by a crash that the restored timer doesn't own.
    pub fn close_orphans(&self, keep: Option<i64>) -> Result<usize> {
        self.conn.execute(
            "UPDATE sessions
             SET status = ?1, ended_at = COALESCE(ended_at, started_at), actual_ms = COALESCE(actual_ms, 0)
             WHERE status = 'running' AND (?2 IS NULL OR id != ?2)",
            params![EndStatus::Abandoned.as_str(), keep],
        )?;
        Ok(self.conn.changes() as usize)
    }

    pub fn sessions_between(&self, from: i64, to: i64) -> Result<Vec<SessionRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, kind, status, started_at, ended_at, planned_ms, COALESCE(actual_ms, 0), pause_count, task
             FROM sessions
             WHERE started_at >= ?1 AND started_at < ?2 AND status NOT IN ('running', 'cancelled')
             ORDER BY started_at",
        )?;
        let rows = stmt.query_map(params![from, to], |r| {
            Ok(SessionRow {
                id: r.get(0)?,
                kind: r.get(1)?,
                status: r.get(2)?,
                started_at: r.get(3)?,
                ended_at: r.get(4)?,
                planned_ms: r.get(5)?,
                actual_ms: r.get(6)?,
                pause_count: r.get(7)?,
                task: r.get(8)?,
            })
        })?;
        rows.collect()
    }

    pub fn day_stats(&self, from: i64, to: i64) -> Result<DayStats> {
        Ok(summarize(self.sessions_between(from, to)?))
    }
}

fn summarize(sessions: Vec<SessionRow>) -> DayStats {
    let mut s = DayStats::default();
    for row in &sessions {
        if row.is_focus() {
            s.focus_sessions += 1;
            s.focus_ms += row.actual_ms;
            s.longest_focus_ms = s.longest_focus_ms.max(row.actual_ms);
            if row.status == EndStatus::Completed.as_str() {
                s.completed += 1;
                s.current_streak += 1;
            } else {
                s.abandoned += 1;
                s.current_streak = 0;
            }
            if row.pause_count > 0 {
                s.interrupted += 1;
            }
        } else {
            s.break_ms += row.actual_ms;
        }
    }
    if s.focus_sessions > 0 {
        s.avg_focus_ms = s.focus_ms / s.focus_sessions as i64;
    }
    s.first_start = sessions.first().map(|r| r.started_at);
    s.last_end = sessions.iter().filter_map(|r| r.ended_at).max();
    s.sessions = sessions;
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timer::MINUTE;

    const T0: i64 = 1_700_000_000_000;

    fn finished(phase: Phase, status: EndStatus, start: i64, mins: i64, pauses: u32) -> Finished {
        Finished {
            session_id: None,
            phase,
            status,
            started_at: start,
            ended_at: start + mins * MINUTE,
            planned_ms: 25 * MINUTE,
            actual_ms: mins * MINUTE,
            pause_count: pauses,
            adjust_ms: 0,
            task: None,
        }
    }

    #[test]
    fn migrations_are_idempotent() {
        let db = Db::open_in_memory().unwrap();
        db.migrate().unwrap();
        let v: usize = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, MIGRATIONS.len());
    }

    #[test]
    fn kv_round_trip() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.kv_get::<Vec<i32>>("x"), None);
        db.kv_set("x", &vec![1, 2]).unwrap();
        db.kv_set("x", &vec![3]).unwrap();
        assert_eq!(db.kv_get::<Vec<i32>>("x"), Some(vec![3]));
    }

    #[test]
    fn running_session_is_updated_in_place() {
        let db = Db::open_in_memory().unwrap();
        let id = db.insert_session(Phase::Focus, T0, 25 * MINUTE, Some("write"), "standard").unwrap();
        assert!(db.sessions_between(T0, T0 + 1).unwrap().is_empty(), "running rows are hidden");
        let mut f = finished(Phase::Focus, EndStatus::Completed, T0, 25, 0);
        f.session_id = Some(id);
        f.task = Some("write".into());
        assert_eq!(db.finish_session(&f, "standard").unwrap(), id);
        let rows = db.sessions_between(T0, T0 + 1).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "completed");
        assert_eq!(rows[0].task.as_deref(), Some("write"));
    }

    #[test]
    fn day_stats_summary() {
        let db = Db::open_in_memory().unwrap();
        let mut t = T0;
        let mut add = |phase, status, mins, pauses| {
            db.finish_session(&finished(phase, status, t, mins, pauses), "standard").unwrap();
            t += 60 * MINUTE;
        };
        add(Phase::Focus, EndStatus::Completed, 25, 0);
        add(Phase::ShortBreak, EndStatus::Completed, 5, 0);
        add(Phase::Focus, EndStatus::Abandoned, 12, 1);
        add(Phase::Focus, EndStatus::Completed, 47, 2);
        add(Phase::Focus, EndStatus::Completed, 25, 0);
        add(Phase::Focus, EndStatus::Cancelled, 0, 0);
        add(Phase::LongBreak, EndStatus::Abandoned, 9, 0);

        let s = db.day_stats(T0, T0 + 24 * 60 * MINUTE).unwrap();
        assert_eq!(s.focus_sessions, 4);
        assert_eq!(s.completed, 3);
        assert_eq!(s.abandoned, 1);
        assert_eq!(s.interrupted, 2);
        assert_eq!(s.focus_ms, 109 * MINUTE);
        assert_eq!(s.break_ms, 14 * MINUTE);
        assert_eq!(s.longest_focus_ms, 47 * MINUTE);
        assert_eq!(s.avg_focus_ms, 109 * MINUTE / 4);
        assert_eq!(s.current_streak, 2);
        assert_eq!(s.sessions.len(), 6);
        assert_eq!(s.first_start, Some(T0));
    }

    #[test]
    fn orphans_are_closed_but_the_live_session_is_kept() {
        let db = Db::open_in_memory().unwrap();
        let a = db.insert_session(Phase::Focus, T0, 25 * MINUTE, None, "standard").unwrap();
        let b = db.insert_session(Phase::Focus, T0 + 1, 25 * MINUTE, None, "standard").unwrap();
        assert_eq!(db.close_orphans(Some(b)).unwrap(), 1);
        let rows = db.sessions_between(T0, T0 + 10).unwrap();
        assert_eq!(rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec![a]);
        assert_eq!(db.close_orphans(None).unwrap(), 1);
    }
}
