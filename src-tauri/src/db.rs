//! Local SQLite storage. Schema changes are appended to `MIGRATIONS` and
//! tracked with `PRAGMA user_version`; never edit a migration once shipped.

use crate::tasks::{self, Bucket, ParsedTask, TaskRow};
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
    // 2 — projects (#tags on the task) and automatic away pauses
    r#"
    ALTER TABLE sessions ADD COLUMN project TEXT;
    ALTER TABLE sessions ADD COLUMN away_count INTEGER NOT NULL DEFAULT 0;
    CREATE INDEX idx_sessions_project ON sessions(project);
    "#,
    // 3 — planner tasks, and focus time credited to them
    r#"
    CREATE TABLE tasks (
      id           INTEGER PRIMARY KEY,
      title        TEXT    NOT NULL,
      project      TEXT,
      bucket       TEXT    NOT NULL,
      position     REAL    NOT NULL,
      estimate_ms  INTEGER,
      status       TEXT    NOT NULL DEFAULT 'open',
      created_at   INTEGER NOT NULL,
      moved_at     INTEGER NOT NULL,
      done_at      INTEGER
    );
    CREATE INDEX idx_tasks_bucket ON tasks(status, bucket, position);
    CREATE INDEX idx_tasks_done   ON tasks(done_at);

    CREATE TABLE session_tasks (
      session_id  INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
      task_id     INTEGER NOT NULL REFERENCES tasks(id)    ON DELETE CASCADE,
      active_ms   INTEGER NOT NULL,
      PRIMARY KEY (session_id, task_id)
    );
    CREATE INDEX idx_session_tasks_task ON session_tasks(task_id);

    ALTER TABLE sessions ADD COLUMN task_id INTEGER REFERENCES tasks(id) ON DELETE SET NULL;
    CREATE INDEX idx_sessions_task ON sessions(task_id);
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
    pub away_count: u32,
    pub task: Option<String>,
    pub project: Option<String>,
    pub rating: Option<u8>,
    pub goal_completed: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistractionRow {
    pub id: i64,
    pub at: i64,
    pub category: Option<String>,
    pub session_id: Option<i64>,
}

pub const DISTRACTION_CATEGORIES: &[&str] =
    &["social", "youtube", "phone", "messaging", "browsing", "work", "other"];

impl SessionRow {
    pub fn is_completed(&self) -> bool {
        self.status == EndStatus::Completed.as_str()
    }

    pub fn is_focus(&self) -> bool {
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
    pub distractions: u32,
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
        project: Option<&str>,
        preset: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO sessions (kind, status, started_at, planned_ms, task, project, preset, created_at)
             VALUES (?1, 'running', ?2, ?3, ?4, ?5, ?6, ?2)",
            params![phase.as_str(), started_at, planned_ms, task, project, preset],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn finish_session(&self, f: &Finished, preset: &str) -> Result<i64> {
        let id = match f.session_id {
            Some(id) => id,
            None => self.insert_session(f.phase, f.started_at, f.planned_ms, f.task.as_deref(), f.project.as_deref(), preset)?,
        };
        self.conn.execute(
            "UPDATE sessions SET status = ?2, ended_at = ?3, planned_ms = ?4, actual_ms = ?5,
                    pause_count = ?6, adjust_ms = ?7, task = ?8, project = ?9, away_count = ?10
             WHERE id = ?1",
            params![
                id,
                f.status.as_str(),
                f.ended_at,
                f.planned_ms,
                f.actual_ms,
                f.pause_count,
                f.adjust_ms,
                f.task,
                f.project,
                f.away_count
            ],
        )?;
        Ok(id)
    }

    pub fn set_session_task(&self, id: i64, task: Option<&str>, project: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE sessions SET task = ?2, project = ?3 WHERE id = ?1", params![id, task, project])?;
        Ok(())
    }

    /// End-of-session reflection. Either field may be left unchanged.
    pub fn rate_session(&self, id: i64, rating: Option<u8>, goal: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET rating = COALESCE(?2, rating), goal_completed = COALESCE(?3, goal_completed) WHERE id = ?1",
            params![id, rating, goal],
        )?;
        Ok(())
    }

    pub fn insert_distraction(&self, at: i64, session_id: Option<i64>) -> Result<i64> {
        self.conn.execute("INSERT INTO distractions (at, session_id) VALUES (?1, ?2)", params![at, session_id])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn set_distraction_category(&self, id: i64, category: Option<&str>) -> Result<()> {
        let category = category.filter(|c| DISTRACTION_CATEGORIES.contains(c));
        self.conn.execute("UPDATE distractions SET category = ?2 WHERE id = ?1", params![id, category])?;
        Ok(())
    }

    pub fn distractions_between(&self, from: i64, to: i64) -> Result<Vec<DistractionRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, at, category, session_id FROM distractions WHERE at >= ?1 AND at < ?2 ORDER BY at",
        )?;
        let rows = stmt.query_map(params![from, to], |r| {
            Ok(DistractionRow { id: r.get(0)?, at: r.get(1)?, category: r.get(2)?, session_id: r.get(3)? })
        })?;
        rows.collect()
    }

    /// Start times of completed focus sessions since `from`, for streaks.
    pub fn completed_focus_starts(&self, from: i64) -> Result<Vec<i64>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT started_at FROM sessions WHERE kind = 'focus' AND status = 'completed' AND started_at >= ?1 ORDER BY started_at",
        )?;
        let rows = stmt.query_map([from], |r| r.get(0))?;
        rows.collect()
    }

    /// Most recently used projects, newest first.
    pub fn recent_projects(&self, limit: u32) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT project FROM sessions WHERE project IS NOT NULL GROUP BY project ORDER BY MAX(started_at) DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| r.get(0))?;
        rows.collect()
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
            "SELECT id, kind, status, started_at, ended_at, planned_ms, COALESCE(actual_ms, 0), pause_count, task,
                    project, away_count, rating, goal_completed
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
                project: r.get(9)?,
                away_count: r.get(10)?,
                rating: r.get(11)?,
                goal_completed: r.get(12)?,
            })
        })?;
        rows.collect()
    }

    pub fn day_stats(&self, from: i64, to: i64) -> Result<DayStats> {
        let mut s = summarize(self.sessions_between(from, to)?);
        s.distractions = self.distractions_between(from, to)?.len() as u32;
        Ok(s)
    }
}

/// Columns read by `task_row`. Spent time is summed from `session_tasks`;
/// the running block's share is added by the engine.
const TASK_COLS: &str = "t.id, t.title, t.project, t.bucket, t.position, t.estimate_ms, t.status, t.created_at,
     t.moved_at, t.done_at, COALESCE((SELECT SUM(active_ms) FROM session_tasks WHERE task_id = t.id), 0)";

fn task_row(r: &rusqlite::Row, today_start: i64) -> rusqlite::Result<TaskRow> {
    let bucket = Bucket::parse(&r.get::<_, String>(3)?).unwrap_or(Bucket::Backlog);
    let done = r.get::<_, String>(6)? == "done";
    let moved_at: i64 = r.get(8)?;
    Ok(TaskRow {
        id: r.get(0)?,
        title: r.get(1)?,
        project: r.get(2)?,
        bucket,
        position: r.get(4)?,
        estimate_ms: r.get(5)?,
        done,
        created_at: r.get(7)?,
        moved_at,
        done_at: r.get(9)?,
        spent_ms: r.get(10)?,
        leftover: tasks::is_leftover(bucket, done, moved_at, today_start),
    })
}

/// Planner tasks. `today_start` (local midnight) only decides `leftover`.
impl Db {
    /// Adds a task at the top or bottom of `bucket`. Returns its id.
    pub fn insert_task(&self, t: &ParsedTask, bucket: Bucket, top: bool, now: i64) -> Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        let first = if top { self.first_task_id(bucket, None)? } else { None };
        let position = self.place(bucket, first, None)?;
        self.conn.execute(
            "INSERT INTO tasks (title, project, bucket, position, estimate_ms, created_at, moved_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![t.title, t.project, bucket.as_str(), position, t.estimate_ms, now],
        )?;
        let id = self.conn.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    pub fn task(&self, id: i64, today_start: i64) -> Result<Option<TaskRow>> {
        self.conn
            .query_row(&format!("SELECT {TASK_COLS} FROM tasks t WHERE t.id = ?1"), [id], |r| task_row(r, today_start))
            .optional()
    }

    /// Every open task, plus tasks done since `done_since`, in bucket order.
    pub fn tasks_list(&self, done_since: i64, today_start: i64) -> Result<Vec<TaskRow>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {TASK_COLS} FROM tasks t
             WHERE t.status = 'open' OR t.done_at >= ?1
             ORDER BY t.bucket, t.position, t.id"
        ))?;
        let rows = stmt.query_map([done_since], |r| task_row(r, today_start))?;
        rows.collect()
    }

    /// Open tasks in one bucket, top first.
    pub fn open_tasks(&self, bucket: Bucket, today_start: i64) -> Result<Vec<TaskRow>> {
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT {TASK_COLS} FROM tasks t
             WHERE t.status = 'open' AND t.bucket = ?1
             ORDER BY t.position, t.id"
        ))?;
        let rows = stmt.query_map([bucket.as_str()], |r| task_row(r, today_start))?;
        rows.collect()
    }

    pub fn update_task(&self, id: i64, t: &ParsedTask) -> Result<()> {
        self.conn.execute(
            "UPDATE tasks SET title = ?2, project = ?3, estimate_ms = ?4 WHERE id = ?1",
            params![id, t.title, t.project, t.estimate_ms],
        )?;
        Ok(())
    }

    /// Moves a task into `bucket` just before `before_id` (None = at the end).
    /// `moved_at` changes only when the bucket does.
    pub fn move_task(&self, id: i64, bucket: Bucket, before_id: Option<i64>, now: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let position = self.place(bucket, before_id.filter(|b| *b != id), Some(id))?;
        self.conn.execute(
            "UPDATE tasks SET position = ?3,
                    moved_at = CASE WHEN bucket = ?2 THEN moved_at ELSE ?4 END,
                    bucket = ?2
             WHERE id = ?1",
            params![id, bucket.as_str(), position, now],
        )?;
        tx.commit()
    }

    /// Ticks a task off, or reopens it at the top of its bucket.
    pub fn set_task_done(&self, id: i64, done: bool, now: i64) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        if done {
            self.conn.execute(
                "UPDATE tasks SET status = 'done', done_at = ?2 WHERE id = ?1 AND status = 'open'",
                params![id, now],
            )?;
        } else {
            let bucket: Option<String> =
                self.conn.query_row("SELECT bucket FROM tasks WHERE id = ?1", [id], |r| r.get(0)).optional()?;
            let Some(bucket) = bucket.as_deref().and_then(Bucket::parse) else { return Ok(()) };
            let position = self.place(bucket, self.first_task_id(bucket, Some(id))?, Some(id))?;
            self.conn.execute(
                "UPDATE tasks SET status = 'open', done_at = NULL, position = ?2 WHERE id = ?1",
                params![id, position],
            )?;
        }
        tx.commit()
    }

    /// Deletes a task and returns it, for undo. Sessions keep their text.
    pub fn delete_task(&self, id: i64, today_start: i64) -> Result<Option<TaskRow>> {
        let row = self.task(id, today_start)?;
        self.conn.execute("DELETE FROM tasks WHERE id = ?1", [id])?;
        Ok(row)
    }

    /// Puts a deleted task back with its id, bucket and position. Its time
    /// credit went with the delete.
    pub fn restore_task(&self, t: &TaskRow) -> Result<()> {
        self.conn.execute(
            "INSERT INTO tasks (id, title, project, bucket, position, estimate_ms, status, created_at, moved_at, done_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                t.id,
                t.title,
                t.project,
                t.bucket.as_str(),
                t.position,
                t.estimate_ms,
                if t.done { "done" } else { "open" },
                t.created_at,
                t.moved_at,
                t.done_at
            ],
        )?;
        Ok(())
    }

    /// Adds focus time from one session to its tasks; repeats are summed.
    pub fn credit_tasks(&self, session_id: i64, credits: &[(i64, i64)]) -> Result<()> {
        let mut stmt = self.conn.prepare_cached(
            "INSERT INTO session_tasks (session_id, task_id, active_ms)
             SELECT ?1, id, ?3 FROM tasks WHERE id = ?2
             ON CONFLICT(session_id, task_id) DO UPDATE SET active_ms = active_ms + excluded.active_ms",
        )?;
        for &(task_id, ms) in credits.iter().filter(|(_, ms)| *ms > 0) {
            stmt.execute(params![session_id, task_id, ms])?;
        }
        Ok(())
    }

    fn first_task_id(&self, bucket: Bucket, except: Option<i64>) -> Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT id FROM tasks WHERE bucket = ?1 AND (?2 IS NULL OR id != ?2) ORDER BY position, id LIMIT 1",
                params![bucket.as_str(), except],
                |r| r.get(0),
            )
            .optional()
    }

    /// A position in `bucket` just before `before_id` (None, or a task not in
    /// the bucket, = the end), ignoring `moving`. Renumbers the bucket when
    /// the gap is too small, so call it inside a transaction.
    fn place(&self, bucket: Bucket, before_id: Option<i64>, moving: Option<i64>) -> Result<f64> {
        for _ in 0..2 {
            let after: Option<f64> = match before_id {
                Some(b) => self
                    .conn
                    .query_row(
                        "SELECT position FROM tasks WHERE id = ?1 AND bucket = ?2",
                        params![b, bucket.as_str()],
                        |r| r.get(0),
                    )
                    .optional()?,
                None => None,
            };
            let before: Option<f64> = self.conn.query_row(
                "SELECT MAX(position) FROM tasks
                 WHERE bucket = ?1 AND (?2 IS NULL OR id != ?2) AND (?3 IS NULL OR position < ?3)",
                params![bucket.as_str(), moving, after],
                |r| r.get(0),
            )?;
            if let Some(p) = tasks::position_between(before, after) {
                return Ok(p);
            }
            self.renumber(bucket)?;
        }
        unreachable!("renumbered positions are 1.0 apart")
    }

    fn renumber(&self, bucket: Bucket) -> Result<()> {
        let ids: Vec<i64> = {
            let mut stmt = self.conn.prepare_cached("SELECT id FROM tasks WHERE bucket = ?1 ORDER BY position, id")?;
            let rows = stmt.query_map([bucket.as_str()], |r| r.get(0))?;
            rows.collect::<Result<_>>()?
        };
        for (i, id) in ids.iter().enumerate() {
            self.conn.execute("UPDATE tasks SET position = ?2 WHERE id = ?1", params![id, (i + 1) as f64])?;
        }
        Ok(())
    }
}

fn summarize(sessions: Vec<SessionRow>) -> DayStats {
    let mut s = DayStats::default();
    for row in &sessions {
        if row.is_focus() {
            s.focus_sessions += 1;
            s.focus_ms += row.actual_ms;
            s.longest_focus_ms = s.longest_focus_ms.max(row.actual_ms);
            if row.is_completed() {
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
            away_count: 0,
            adjust_ms: 0,
            task: None,
            project: None,
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
        let id = db.insert_session(Phase::Focus, T0, 25 * MINUTE, Some("write"), Some("blog"), "standard").unwrap();
        assert!(db.sessions_between(T0, T0 + 1).unwrap().is_empty(), "running rows are hidden");
        let mut f = finished(Phase::Focus, EndStatus::Completed, T0, 25, 0);
        f.session_id = Some(id);
        f.task = Some("write".into());
        f.project = Some("blog".into());
        assert_eq!(db.finish_session(&f, "standard").unwrap(), id);
        let rows = db.sessions_between(T0, T0 + 1).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "completed");
        assert_eq!(rows[0].task.as_deref(), Some("write"));
        assert_eq!(rows[0].project.as_deref(), Some("blog"));

        db.rate_session(id, Some(4), None).unwrap();
        db.rate_session(id, None, Some("yes")).unwrap();
        let row = &db.sessions_between(T0, T0 + 1).unwrap()[0];
        assert_eq!((row.rating, row.goal_completed.as_deref()), (Some(4), Some("yes")));
    }

    #[test]
    fn distractions_are_logged_and_categorised() {
        let db = Db::open_in_memory().unwrap();
        let a = db.insert_distraction(T0, None).unwrap();
        let b = db.insert_distraction(T0 + 5, None).unwrap();
        db.set_distraction_category(a, Some("youtube")).unwrap();
        db.set_distraction_category(b, Some("not-a-category")).unwrap();
        let rows = db.distractions_between(T0, T0 + 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].category.as_deref(), Some("youtube"));
        assert_eq!(rows[1].category, None);
        assert_eq!(db.day_stats(T0, T0 + 10).unwrap().distractions, 2);
    }

    #[test]
    fn recent_projects_newest_first() {
        let db = Db::open_in_memory().unwrap();
        for (i, p) in ["a", "b", "a", "c"].iter().enumerate() {
            db.insert_session(Phase::Focus, T0 + i as i64, MINUTE, None, Some(p), "standard").unwrap();
        }
        assert_eq!(db.recent_projects(2).unwrap(), vec!["c".to_string(), "a".to_string()]);
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
        let a = db.insert_session(Phase::Focus, T0, 25 * MINUTE, None, None, "standard").unwrap();
        let b = db.insert_session(Phase::Focus, T0 + 1, 25 * MINUTE, None, None, "standard").unwrap();
        assert_eq!(db.close_orphans(Some(b)).unwrap(), 1);
        let rows = db.sessions_between(T0, T0 + 10).unwrap();
        assert_eq!(rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec![a]);
        assert_eq!(db.close_orphans(None).unwrap(), 1);
    }

    fn add(db: &Db, text: &str, bucket: Bucket, top: bool) -> i64 {
        db.insert_task(&tasks::parse(text).unwrap(), bucket, top, T0).unwrap()
    }

    fn titles(db: &Db, bucket: Bucket) -> Vec<String> {
        db.open_tasks(bucket, T0).unwrap().into_iter().map(|t| t.title).collect()
    }

    #[test]
    fn migration_3_tasks_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let id = add(&db, "Prep slides 28min #job-search", Bucket::Today, false);
        let t = db.task(id, T0).unwrap().unwrap();
        assert_eq!((t.title.as_str(), t.project.as_deref()), ("Prep slides", Some("job-search")));
        assert_eq!((t.bucket, t.estimate_ms, t.done, t.spent_ms), (Bucket::Today, Some(28 * MINUTE), false, 0));
        assert!(!t.leftover);
        assert!(db.task(id, T0 + 1).unwrap().unwrap().leftover, "added before today started");

        db.update_task(id, &tasks::parse("Prep deck 1h").unwrap()).unwrap();
        let t = db.task(id, T0).unwrap().unwrap();
        assert_eq!((t.title.as_str(), t.project, t.estimate_ms), ("Prep deck", None, Some(60 * MINUTE)));
    }

    #[test]
    fn migration_3_insert_top_and_bottom() {
        let db = Db::open_in_memory().unwrap();
        add(&db, "b", Bucket::Today, false);
        add(&db, "c", Bucket::Today, false);
        add(&db, "a", Bucket::Today, true);
        add(&db, "elsewhere", Bucket::Week, true);
        assert_eq!(titles(&db, Bucket::Today), ["a", "b", "c"]);
        assert_eq!(titles(&db, Bucket::Week), ["elsewhere"]);
    }

    #[test]
    fn migration_3_move_within_and_across_buckets() {
        let db = Db::open_in_memory().unwrap();
        let a = add(&db, "a", Bucket::Today, false);
        let b = add(&db, "b", Bucket::Today, false);
        let c = add(&db, "c", Bucket::Today, false);
        let w = add(&db, "w", Bucket::Week, false);

        db.move_task(c, Bucket::Today, Some(a), T0 + 5).unwrap();
        assert_eq!(titles(&db, Bucket::Today), ["c", "a", "b"]);
        assert_eq!(db.task(c, T0).unwrap().unwrap().moved_at, T0, "same bucket keeps moved_at");

        db.move_task(a, Bucket::Week, Some(w), T0 + 5).unwrap();
        assert_eq!(titles(&db, Bucket::Week), ["a", "w"]);
        assert_eq!(db.task(a, T0).unwrap().unwrap().moved_at, T0 + 5);

        db.move_task(w, Bucket::Today, None, T0 + 6).unwrap();
        db.move_task(c, Bucket::Today, Some(c), T0 + 6).unwrap();
        assert_eq!(titles(&db, Bucket::Today), ["b", "w", "c"], "before itself = to the end");
        db.move_task(b, Bucket::Today, Some(9999), T0 + 6).unwrap();
        assert_eq!(titles(&db, Bucket::Today), ["w", "c", "b"], "unknown neighbour = to the end");
    }

    #[test]
    fn migration_3_positions_are_renumbered_when_the_gap_runs_out() {
        let db = Db::open_in_memory().unwrap();
        let a = add(&db, "a", Bucket::Today, false);
        let b = add(&db, "b", Bucket::Today, false);
        let mut moved = Vec::new();
        // Each move halves the gap in front of b. Without renumbering, floats
        // run out of room after ~52 halvings and the order breaks.
        for i in 0..80 {
            let id = add(&db, &format!("t{i}"), Bucket::Backlog, false);
            db.move_task(id, Bucket::Today, Some(b), T0).unwrap();
            moved.push(format!("t{i}"));
        }
        let mut want = vec!["a".to_string()];
        want.extend(moved);
        want.push("b".into());
        assert_eq!(titles(&db, Bucket::Today), want);
        let rows = db.open_tasks(Bucket::Today, T0).unwrap();
        assert!(rows.windows(2).all(|w| w[1].position > w[0].position));
        assert_eq!(rows[0].id, a);
    }

    #[test]
    fn migration_3_done_and_reopen() {
        let db = Db::open_in_memory().unwrap();
        let a = add(&db, "a", Bucket::Today, false);
        add(&db, "b", Bucket::Today, false);
        db.set_task_done(a, true, T0 + 9).unwrap();
        assert_eq!(titles(&db, Bucket::Today), ["b"]);
        let done = db.task(a, T0).unwrap().unwrap();
        assert_eq!((done.done, done.done_at, done.bucket), (true, Some(T0 + 9), Bucket::Today));
        assert!(db.tasks_list(T0 + 10, T0).unwrap().iter().all(|t| t.id != a), "done before the window");
        assert!(db.tasks_list(T0 + 9, T0).unwrap().iter().any(|t| t.id == a));

        add(&db, "c", Bucket::Today, true);
        db.set_task_done(a, false, T0 + 10).unwrap();
        assert_eq!(titles(&db, Bucket::Today), ["a", "c", "b"], "reopened at the top");
        assert_eq!(db.task(a, T0).unwrap().unwrap().done_at, None);
    }

    #[test]
    fn migration_3_credit_is_summed_and_delete_keeps_sessions() {
        let db = Db::open_in_memory().unwrap();
        let a = add(&db, "a", Bucket::Today, false);
        let b = add(&db, "b", Bucket::Today, false);
        let s = db.insert_session(Phase::Focus, T0, 25 * MINUTE, Some("a"), None, "standard").unwrap();
        db.credit_tasks(s, &[(a, 10 * MINUTE), (b, 15 * MINUTE), (a, 2 * MINUTE), (b, 0), (424242, MINUTE)]).unwrap();
        db.conn.execute("UPDATE sessions SET task_id = ?1 WHERE id = ?2", params![a, s]).unwrap();
        assert_eq!(db.task(a, T0).unwrap().unwrap().spent_ms, 12 * MINUTE);
        assert_eq!(db.task(b, T0).unwrap().unwrap().spent_ms, 15 * MINUTE);

        let gone = db.delete_task(a, T0).unwrap().unwrap();
        assert_eq!(db.task(a, T0).unwrap(), None);
        let (task_id, task): (Option<i64>, Option<String>) =
            db.conn.query_row("SELECT task_id, task FROM sessions WHERE id = ?1", [s], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((task_id, task.as_deref()), (None, Some("a")), "session kept, text kept");
        let credits: i64 = db.conn.query_row("SELECT COUNT(*) FROM session_tasks", [], |r| r.get(0)).unwrap();
        assert_eq!(credits, 1, "only b's credit is left");

        db.restore_task(&gone).unwrap();
        let back = db.task(a, T0).unwrap().unwrap();
        assert_eq!((back.title, back.position, back.spent_ms), (gone.title, gone.position, 0));
        assert_eq!(db.delete_task(9999, T0).unwrap(), None);
    }
}
