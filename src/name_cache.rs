//! Derived session metadata; transcripts remain the source of truth. -- PI/OpenAI

use crate::sessions::Row;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub struct NameCache {
    path: PathBuf,
    entries: HashMap<String, Value>,
    changed: bool,
}

fn stamp(path: &Path) -> Option<Value> {
    let metadata = path.metadata().ok()?;
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(json!([
        metadata.len(),
        modified.as_secs(),
        modified.subsec_nanos()
    ]))
}

impl NameCache {
    pub fn load() -> Self {
        let root = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| crate::sessions::home().join(".cache"));
        let path = root.join("asf/names-v1.json");
        let entries = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(entries) => entries,
                Err(error) => {
                    eprintln!("asf: rebuilding invalid metadata cache: {error}");
                    HashMap::new()
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(error) => {
                eprintln!("asf: cannot read metadata cache: {error}");
                HashMap::new()
            }
        };
        Self {
            path,
            entries,
            changed: false,
        }
    }

    pub fn get(&self, path: &Path, agent: &str) -> Option<Vec<Row>> {
        let key = path.to_string_lossy();
        let entry = self.entries.get(key.as_ref())?;
        if entry["stamp"] != stamp(path)? {
            return None;
        }
        let rows = entry["rows"].as_array()?;
        rows.iter()
            .map(|row| {
                Some(Row {
                    path: key.to_string(),
                    hit: key.to_string(),
                    agent: agent.to_string(),
                    cwd: row["cwd"].as_str()?.to_string(),
                    title: row["title"].as_str()?.to_string(),
                    opening: row["opening"].as_str()?.to_string(),
                    line: row["line"].as_u64()?,
                    sub: row["sub"].as_bool()?,
                    ..Row::default()
                })
            })
            .collect()
    }

    pub fn put(&mut self, path: &Path, rows: &[Row], before: &Value) {
        // Do not cache a partial read while an agent appends or rewrites the transcript. -- PI/OpenAI
        if stamp(path).as_ref() != Some(before) {
            return;
        }
        let rows: Vec<_> = rows
            .iter()
            .map(|row| {
                json!({
                    "cwd": row.cwd, "title": row.title, "opening": row.opening,
                    "line": row.line, "sub": row.sub,
                })
            })
            .collect();
        self.entries.insert(
            path.to_string_lossy().into_owned(),
            json!({"stamp": before, "rows": rows}),
        );
        self.changed = true;
    }

    pub fn stamp(path: &Path) -> Option<Value> {
        stamp(path)
    }
}

impl Drop for NameCache {
    fn drop(&mut self) {
        if !self.changed {
            return;
        }
        let save = || -> std::io::Result<()> {
            std::fs::create_dir_all(self.path.parent().unwrap())?;
            let temporary = self
                .path
                .with_extension(format!("{}.tmp", std::process::id()));
            std::fs::write(&temporary, serde_json::to_vec(&self.entries)?)?;
            std::fs::rename(temporary, &self.path)
        };
        if let Err(error) = save() {
            eprintln!("asf: cannot save metadata cache: {error}");
        }
    }
}
