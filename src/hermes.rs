//! hermes keeps its sessions in ~/.hermes/state.db, sqlite, not in files. Read it with the
//! sqlite3 command: one process, json out, and nothing to build.
//!
//! A hermes session has no file of its own, so its path is `<db>#<session id>`.

use crate::record::{Show, clean, window};
use crate::sessions::{Row, home};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

static WARNED: std::sync::OnceLock<()> = std::sync::OnceLock::new();

pub fn db() -> PathBuf {
    home().join(".hermes/state.db")
}

pub fn path_of(id: &str) -> String {
    format!("{}#{id}", db().display())
}

pub fn id_of(path: &str) -> String {
    // an id hermes wrote, and nothing else, because it goes into a SQL string below
    let id = path.rsplit('#').next().unwrap_or_default();
    assert!(
        id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "not a hermes session id: {id:?}"
    );
    id.to_string()
}

/// Rows of the query, as json. Empty when there is no db to read.
fn query(sql: &str) -> Vec<Value> {
    if !db().exists() {
        return Vec::new();
    }
    // mode=ro reads the write-ahead log too; immutable=1 skips it and loses the newest rows
    let uri = format!("file:{}?mode=ro", db().display());
    // one query per call, and a listing calls this three times, so say it once
    let once = |said: String| std::sync::OnceLock::get_or_init(&WARNED, || eprintln!("{said}"));
    let out = match Command::new("sqlite3").args(["-json", &uri, sql]).output() {
        Ok(out) => out,
        Err(err) => {
            once(format!("asf: hermes needs the sqlite3 command: {err}"));
            return Vec::new();
        }
    };
    if !out.status.success() {
        once(format!("asf: hermes: {}", String::from_utf8_lossy(&out.stderr).trim()));
        return Vec::new();
    }
    serde_json::from_slice(&out.stdout).unwrap_or_default()
}

fn text(row: &Value, key: &str) -> String {
    row.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// Every hermes session. The name it was given, else the first thing you said to it.
pub fn sessions() -> Vec<Row> {
    let sql = "select s.id, coalesce(nullif(s.title,''), nullif(s.display_name,''), '') as name,
        coalesce(s.cwd, '') as cwd, coalesce(s.ended_at, s.started_at) as at,
        coalesce(s.parent_session_id, '') as parent,
        coalesce((select m.content from messages m where m.session_id = s.id
                  and m.role = 'user' order by m.timestamp limit 1), '') as opening
        from sessions s";
    query(sql)
        .iter()
        .map(|row| {
            let opening = clean(&text(row, "opening"), 110);
            let name = clean(&text(row, "name"), 110);
            Row {
                path: path_of(&text(row, "id")),
                agent: "hermes".to_string(),
                cwd: text(row, "cwd"),
                title: if name.is_empty() { opening.clone() } else { name },
                opening,
                mtime: row.get("at").and_then(Value::as_f64).unwrap_or_default(),
                // hermes records the session that spawned this one, which is the whole rule
                sub: !text(row, "parent").is_empty(),
                ..Row::default()
            }
        })
        .collect()
}

pub fn session_of(path: &str) -> Option<Row> {
    sessions().into_iter().find(|row| row.path == path)
}

/// Sessions whose messages match. 500 kB of text in the whole table, so read it all and let
/// the same regex decide, instead of teaching SQL about the query.
pub fn search_content(pattern: &regex::Regex, query_text: &str) -> Vec<Row> {
    let said = query(
        "select session_id, content from messages where content is not null order by timestamp",
    );
    let mut hit: Vec<(String, String)> = Vec::new();
    for row in &said {
        let content = text(row, "content");
        if pattern.is_match(&content) {
            let id = text(row, "session_id");
            if !hit.iter().any(|(seen, _)| *seen == id) {
                hit.push((id, content));
            }
        }
    }
    let mut rows = sessions();
    rows.retain(|row| hit.iter().any(|(id, _)| path_of(id) == row.path));
    for row in rows.iter_mut() {
        let (_, said) = hit.iter().find(|(id, _)| path_of(id) == row.path).unwrap();
        row.matched = window(said, query_text, 110);
    }
    rows
}

/// One block per message, the same shape the jsonl agents get.
pub fn blocks(path: &str, show: Show) -> Vec<String> {
    let sql = format!(
        "select role, coalesce(content,'') as content, coalesce(tool_name,'') as tool_name,
         coalesce(tool_calls,'') as tool_calls, coalesce(reasoning,'') as reasoning
         from messages where session_id = '{}' order by timestamp, id",
        id_of(path)
    );
    query(&sql)
        .iter()
        .filter_map(|row| {
            let mut said = vec![text(row, "content")];
            if show.tools {
                let calls = text(row, "tool_calls");
                if !calls.is_empty() {
                    // the name is on the call, and only a tool result carries tool_name
                    let mut name = text(row, "tool_name");
                    if name.is_empty() {
                        name = crate::record::parse(&calls)
                            .map_or(String::new(), |v| crate::record::find_value(&v, "name"));
                    }
                    said.push(format!("- `{name}` {}", clean(&calls, 200)));
                }
            }
            if show.think {
                let thought = text(row, "reasoning");
                if !thought.is_empty() {
                    said.push(format!("> {}", clean(&thought, 4000)));
                }
            }
            said.retain(|part| !part.trim().is_empty());
            if said.is_empty() {
                return None;
            }
            Some(format!("# {}\n{}", text(row, "role"), said.join("\n")))
        })
        .collect()
}
