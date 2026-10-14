//! `chronos mcp`: a Model Context Protocol server on stdin/stdout, so Claude
//! Code, in any project, can put things only you can do into Chronos. It runs
//! without the app: it opens the same database, and a running Chronos picks
//! the change up within a couple of seconds (`Db::changed_elsewhere`).
//! Claude sees, moves and ticks off only the to-dos it added (`source = "claude"`).
//! See docs/CLAUDE-CODE.md.

use crate::db::Db;
use crate::engine::{day_bounds, now_ms};
use crate::tasks::{self, Bucket, TaskRow};
use crate::timer::MINUTE;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;

pub const SOURCE: &str = "claude";
const MAX_NOTE: usize = 1000;
/// Answered when the client doesn't say which version it speaks.
const PROTOCOL: &str = "2025-06-18";
const DAY: i64 = 86_400_000;

const INSTRUCTIONS: &str = "Chronos is the user's task list and focus timer. When the user must do something \
themselves outside this chat (log in, add a key or secret, pay, approve, test on a device, reply to someone), \
add it with add_todo: a short imperative title, and a note with the exact command, file or link. Check \
list_todos first so you don't add the same thing twice, and tick one off with complete_todo once you can see \
it's done. Don't add ordinary questions you're asking in the chat.";

/// Serve until stdin closes. Returns the process exit code.
pub fn serve() -> i32 {
    let db = match open_db() {
        Ok(db) => db,
        Err(e) => {
            eprintln!("chronos mcp: {e}");
            return 1;
        }
    };
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(&db, &msg),
            Err(_) => Some(error(Value::Null, -32700, "Parse error")),
        };
        if let Some(reply) = reply {
            let _ = writeln!(out, "{reply}");
            let _ = out.flush();
        }
    }
    0
}

/// The app's database: ~/Library/Application Support/com.chronos.desk.
/// `CHRONOS_DB` points somewhere else (tests).
fn open_db() -> Result<Db, String> {
    let path = match std::env::var_os("CHRONOS_DB") {
        Some(p) => PathBuf::from(p),
        None => {
            let home = std::env::var_os("HOME").ok_or("HOME isn't set")?;
            let dir = PathBuf::from(home).join("Library/Application Support/com.chronos.desk");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            dir.join("chronos.sqlite3")
        }
    };
    Db::open(&path).map_err(|e| format!("can't open {}: {e}", path.display()))
}

/// One JSON-RPC message in, at most one out. Notifications get no reply.
pub fn handle(db: &Db, msg: &Value) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let method = msg["method"].as_str().unwrap_or("");
    let result = match method {
        "initialize" => json!({
            "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "chronos", "title": "Chronos", "version": env!("CARGO_PKG_VERSION") },
            "instructions": INSTRUCTIONS,
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools() }),
        "tools/call" => {
            let params = &msg["params"];
            match call(db, params["name"].as_str().unwrap_or(""), &params["arguments"], now_ms()) {
                Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
                Err(text) => json!({ "content": [{ "type": "text", "text": text }], "isError": true }),
            }
        }
        _ => return Some(error(id, -32601, &format!("Unknown method: {method}"))),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn tools() -> Value {
    json!([
        {
            "name": "add_todo",
            "title": "Add a to-do for the user",
            "description": "Put something only the user can do (outside this chat) at the top of a list in Chronos \
                (Today unless the user asks for This week or the Backlog), tagged with this project. Returns the to-do's id. \
                If the same open to-do exists, returns that one instead.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Short and imperative, e.g. \"Log in to the GitHub CLI\". At most 120 characters." },
                    "note": { "type": "string", "description": "The details: the exact command, file, link, or why it's needed." },
                    "project": { "type": "string", "description": "Project tag without #. Defaults to this project's folder name." },
                    "estimate_minutes": { "type": "integer", "minimum": 1, "maximum": 480, "description": "Rough time it'll take the user, if you can tell." },
                    "list": list_schema()
                },
                "required": ["title"]
            }
        },
        {
            "name": "list_todos",
            "title": "List Claude's to-dos",
            "description": "The to-dos Claude added to Chronos that are still open, with the list each is in (and, with include_done, those finished in the last 7 days). Never shows the user's own tasks.",
            "inputSchema": {
                "type": "object",
                "properties": { "include_done": { "type": "boolean", "description": "Also list ones finished in the last 7 days." } }
            },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "move_todo",
            "title": "Move a to-do to another list",
            "description": "Move one of Claude's to-dos to the top of Today, This week or the Backlog. Only works on to-dos Claude added.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "The id add_todo or list_todos gave." },
                    "list": list_schema()
                },
                "required": ["id", "list"]
            }
        },
        {
            "name": "complete_todo",
            "title": "Tick off a to-do",
            "description": "Mark one of Claude's to-dos done, once you can see the user has done it. Only works on to-dos Claude added.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "integer", "description": "The id add_todo or list_todos gave." } },
                "required": ["id"]
            }
        }
    ])
}

/// The three lists of the Plan tab, as Claude names them.
fn list_schema() -> Value {
    json!({
        "type": "string",
        "enum": ["today", "this_week", "backlog"],
        "description": "Which list: today (the default), this_week or backlog."
    })
}

fn parse_list(v: &Value) -> Result<Bucket, String> {
    match v.as_str().map(|s| s.trim().to_ascii_lowercase().replace([' ', '-'], "_")).as_deref() {
        None | Some("") | Some("today") => Ok(Bucket::Today),
        Some("this_week" | "week") => Ok(Bucket::Week),
        Some("backlog") => Ok(Bucket::Backlog),
        Some(other) => Err(format!("There's no \"{other}\" list; use today, this_week or backlog.")),
    }
}

/// How the Plan tab names a list.
fn list_name(b: Bucket) -> &'static str {
    match b {
        Bucket::Today => "Today",
        Bucket::Week => "This week",
        Bucket::Backlog => "Backlog",
    }
}

/// Claude's own to-do `id`, or why it can't be touched.
fn own_todo(db: &Db, id: i64, today: i64) -> Result<TaskRow, String> {
    let t = db
        .task(id, today)
        .map_err(|e| format!("Chronos couldn't read that: {e}"))?
        .ok_or(format!("There's no to-do #{id} in Chronos."))?;
    if t.source.as_deref() != Some(SOURCE) {
        return Err(format!("#{id} is one of the user's own tasks, not Claude's; only they can change it."));
    }
    Ok(t)
}

fn call(db: &Db, name: &str, args: &Value, now: i64) -> Result<String, String> {
    let today = day_bounds(now).0;
    let db_err = |e: rusqlite::Error| format!("Chronos couldn't save that: {e}");
    match name {
        "add_todo" => {
            let mut task = tasks::parse(args["title"].as_str().unwrap_or("")).ok_or("A to-do needs a title.")?;
            match args["project"].as_str() {
                Some(p) => task.project = tag(p),
                None if task.project.is_none() => task.project = folder_project(),
                None => {}
            }
            let bucket = parse_list(&args["list"])?;
            if let Some(m) = args["estimate_minutes"].as_i64() {
                task.estimate_ms = Some(m.clamp(1, 480) * MINUTE);
            }
            let note: Option<String> = args["note"]
                .as_str()
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(|n| n.chars().take(MAX_NOTE).collect());
            // The same open to-do already there: say so instead of adding it twice.
            let open = db.tasks_from(SOURCE, i64::MAX, today).map_err(db_err)?;
            if let Some(t) = open.iter().find(|t| t.title.eq_ignore_ascii_case(&task.title)) {
                return Ok(format!(
                    "Already in Chronos as #{} in {}: {} (move_todo moves it)",
                    t.id,
                    list_name(t.bucket),
                    t.title
                ));
            }
            let id = db.insert_task_from(&task, bucket, true, now, note.as_deref(), Some(SOURCE)).map_err(db_err)?;
            let tag = task.project.map(|p| format!(" (#{p})")).unwrap_or_default();
            Ok(format!("Added to the top of {} in Chronos as #{id}: {}{tag}", list_name(bucket), task.title))
        }
        "list_todos" => {
            let since = if args["include_done"].as_bool().unwrap_or(false) { today - 6 * DAY } else { i64::MAX };
            let rows = db.tasks_from(SOURCE, since, today).map_err(db_err)?;
            if rows.is_empty() {
                return Ok("No open to-dos from Claude in Chronos.".into());
            }
            Ok(rows.iter().map(describe).collect::<Vec<_>>().join("\n"))
        }
        "move_todo" => {
            let id = args["id"].as_i64().ok_or("Give the to-do's id.")?;
            let bucket = parse_list(&args["list"])?;
            let t = own_todo(db, id, today)?;
            if t.done {
                return Err(format!("#{id} is already done: {}", t.title));
            }
            let top = db.open_tasks(bucket, today).map_err(db_err)?.into_iter().map(|t| t.id).find(|&o| o != id);
            db.move_task(id, bucket, top, now).map_err(db_err)?;
            Ok(format!("Moved #{id} to the top of {} in Chronos: {}", list_name(bucket), t.title))
        }
        "complete_todo" => {
            let id = args["id"].as_i64().ok_or("Give the to-do's id.")?;
            let t = own_todo(db, id, today)?;
            if t.done {
                return Ok(format!("#{id} is already done: {}", t.title));
            }
            db.set_task_done(id, true, now).map_err(db_err)?;
            Ok(format!("Ticked off #{id} in Chronos: {}", t.title))
        }
        _ => Err(format!("Unknown tool: {name}")),
    }
}

/// "#12 · Today · Log in to the GitHub CLI · #chronos" plus the note.
fn describe(t: &TaskRow) -> String {
    let mut line = format!("#{} · {} · {}", t.id, if t.done { "done" } else { list_name(t.bucket) }, t.title);
    if let Some(p) = &t.project {
        line += &format!(" · #{p}");
    }
    if let Some(n) = &t.note {
        line += &format!("\n    {}", n.replace('\n', "\n    "));
    }
    line
}

/// "Job Search" → "job-search", as the Plan tab would read "#job-search".
fn tag(text: &str) -> Option<String> {
    tasks::split_tags(&format!("#{}", text.trim().trim_start_matches('#').replace(' ', "-"))).1
}

/// The folder Claude Code is working in, as a project tag. None in $HOME.
fn folder_project() -> Option<String> {
    let dir = std::env::var_os("CLAUDE_PROJECT_DIR").map(PathBuf::from).or_else(|| std::env::current_dir().ok())?;
    if std::env::var_os("HOME").is_some_and(|h| PathBuf::from(h) == dir) {
        return None;
    }
    tag(&dir.file_name()?.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_760_000_000_000;

    fn rpc(db: &Db, method: &str, params: Value) -> Value {
        handle(db, &json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })).unwrap()
    }

    fn tool(db: &Db, name: &str, args: Value) -> Result<String, String> {
        call(db, name, &args, NOW)
    }

    #[test]
    fn handshake_and_tool_list() {
        let db = Db::open_in_memory().unwrap();
        let init = rpc(&db, "initialize", json!({ "protocolVersion": "2025-03-26" }));
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(init["result"]["serverInfo"]["name"], "chronos");
        assert!(handle(&db, &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
        let names: Vec<_> = rpc(&db, "tools/list", json!({}))["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, ["add_todo", "list_todos", "move_todo", "complete_todo"]);
        assert_eq!(rpc(&db, "nope", json!({}))["error"]["code"], -32601);
    }

    #[test]
    fn adds_to_the_top_of_today_with_a_note() {
        let db = Db::open_in_memory().unwrap();
        db.insert_task(&tasks::parse("Mine").unwrap(), Bucket::Today, false, NOW).unwrap();
        let out = tool(&db, "add_todo", json!({ "title": "Log in to the GitHub CLI", "note": "Run `gh auth login`", "project": "Job Search", "estimate_minutes": 5 })).unwrap();
        assert!(out.contains("#job-search"), "{out}");
        let today = db.tasks_list(0, day_bounds(NOW).0).unwrap();
        let top = today.iter().filter(|t| t.bucket == Bucket::Today).min_by(|a, b| a.position.total_cmp(&b.position)).unwrap();
        assert_eq!(top.title, "Log in to the GitHub CLI");
        assert_eq!(top.note.as_deref(), Some("Run `gh auth login`"));
        assert_eq!(top.source.as_deref(), Some(SOURCE));
        assert_eq!(top.estimate_ms, Some(5 * MINUTE));
    }

    fn top_of(db: &Db, bucket: Bucket) -> TaskRow {
        db.open_tasks(bucket, day_bounds(NOW).0).unwrap().into_iter().next().unwrap()
    }

    #[test]
    fn adds_to_this_week_or_the_backlog_when_asked() {
        let db = Db::open_in_memory().unwrap();
        db.insert_task(&tasks::parse("Mine").unwrap(), Bucket::Week, false, NOW).unwrap();
        let out = tool(&db, "add_todo", json!({ "title": "Review the Revlon deck", "project": "revlon", "list": "this_week" })).unwrap();
        assert!(out.contains("top of This week"), "{out}");
        assert_eq!(top_of(&db, Bucket::Week).title, "Review the Revlon deck");
        tool(&db, "add_todo", json!({ "title": "Read the brand guide", "list": "Backlog" })).unwrap();
        assert_eq!(top_of(&db, Bucket::Backlog).title, "Read the brand guide");
        assert!(db.open_tasks(Bucket::Today, day_bounds(NOW).0).unwrap().is_empty());
        assert!(tool(&db, "add_todo", json!({ "title": "X", "list": "someday" })).is_err());
    }

    #[test]
    fn moves_only_its_own_todos() {
        let db = Db::open_in_memory().unwrap();
        db.insert_task(&tasks::parse("Mine").unwrap(), Bucket::Week, false, NOW).unwrap();
        tool(&db, "add_todo", json!({ "title": "Send the brief", "project": "revlon" })).unwrap();
        let id = top_of(&db, Bucket::Today).id;
        let out = tool(&db, "move_todo", json!({ "id": id, "list": "this_week" })).unwrap();
        assert!(out.starts_with("Moved"), "{out}");
        assert_eq!(top_of(&db, Bucket::Week).id, id);
        assert!(tool(&db, "list_todos", json!({})).unwrap().contains("This week · Send the brief"));
        let mine = db.open_tasks(Bucket::Week, day_bounds(NOW).0).unwrap()[1].id;
        assert!(tool(&db, "move_todo", json!({ "id": mine, "list": "backlog" })).is_err());
        let again = tool(&db, "add_todo", json!({ "title": "send the brief" })).unwrap();
        assert!(again.contains("in This week"), "{again}");
    }

    #[test]
    fn same_open_todo_is_not_added_twice() {
        let db = Db::open_in_memory().unwrap();
        tool(&db, "add_todo", json!({ "title": "Add the Stripe key", "project": "shop" })).unwrap();
        let again = tool(&db, "add_todo", json!({ "title": "add the stripe key" })).unwrap();
        assert!(again.starts_with("Already in Chronos"), "{again}");
        assert_eq!(db.tasks_from(SOURCE, i64::MAX, 0).unwrap().len(), 1);
    }

    #[test]
    fn lists_and_ticks_off_only_its_own() {
        let db = Db::open_in_memory().unwrap();
        let mine = db.insert_task(&tasks::parse("My own task").unwrap(), Bucket::Today, false, NOW).unwrap();
        tool(&db, "add_todo", json!({ "title": "Approve the Apple email", "note": "In Gmail", "project": "x" })).unwrap();
        let list = tool(&db, "list_todos", json!({})).unwrap();
        assert!(list.contains("Approve the Apple email") && list.contains("In Gmail") && !list.contains("My own task"), "{list}");

        let id = db.tasks_from(SOURCE, i64::MAX, 0).unwrap()[0].id;
        assert!(tool(&db, "complete_todo", json!({ "id": mine })).is_err());
        assert!(tool(&db, "complete_todo", json!({ "id": id })).unwrap().starts_with("Ticked off"));
        assert_eq!(tool(&db, "list_todos", json!({})).unwrap(), "No open to-dos from Claude in Chronos.");
        assert!(tool(&db, "list_todos", json!({ "include_done": true })).unwrap().contains("done · Approve"));
        assert!(tool(&db, "complete_todo", json!({ "id": 999 })).is_err());
    }

    #[test]
    fn errors_are_tool_results_not_crashes() {
        let db = Db::open_in_memory().unwrap();
        let r = rpc(&db, "tools/call", json!({ "name": "add_todo", "arguments": { "title": "  " } }));
        assert_eq!(r["result"]["isError"], true);
    }
}
