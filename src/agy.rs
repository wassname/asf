//! Read agy-reader trajectory exports without running agy-reader. -- PI/OpenAI

use crate::record::{Show, UUID, clean, is_junk, window};
use crate::sessions::{Row, home, mtime};
use crate::transcript::{Message, Source};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const CLI: &str = ".gemini/antigravity-cli/conversations";
pub const IDE: &str = ".gemini/antigravity/conversations";
const SUFFIX: &str = ".trajectory.json";

pub fn id(path: &Path) -> Option<&str> {
    let id = path.file_name()?.to_str()?.strip_suffix(SUFFIX)?;
    UUID.is_match(id).then_some(id).filter(|id| id.len() == 36)
}

fn conversation_files() -> impl Iterator<Item = PathBuf> {
    [CLI, IDE]
        .into_iter()
        .flat_map(|root| {
            std::fs::read_dir(home().join(root))
                .into_iter()
                .flatten()
                .flatten()
        })
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path())
}

pub fn paths() -> Vec<PathBuf> {
    conversation_files()
        .filter(|path| id(path).is_some())
        .collect()
}

pub fn native_summary() -> (usize, Option<f64>) {
    let files = conversation_files()
        .filter(|path| matches!(path.extension().and_then(|s| s.to_str()), Some("db" | "pb")))
        .filter(|path| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.len() == 36 && UUID.is_match(s))
        })
        .collect::<Vec<_>>();
    let count = files
        .iter()
        .map(|path| path.with_extension(""))
        .collect::<std::collections::HashSet<_>>()
        .len();
    let latest = files
        .iter()
        .map(|path| mtime(&path.to_string_lossy()))
        .reduce(f64::max);
    (count, latest)
}

fn load(path: &str) -> Result<Value, String> {
    let file = std::fs::File::open(path).map_err(|error| format!("{path}: {error}"))?;
    let root: Value = serde_json::from_reader(std::io::BufReader::new(file))
        .map_err(|error| format!("{path}: invalid Antigravity export: {error}"))?;
    if !root.is_object()
        || root.get("cascadeId").and_then(Value::as_str).is_none()
        || root.get("steps").is_some_and(|steps| !steps.is_array())
    {
        return Err(format!(
            "{path}: expected an agy-reader trajectory with a cascadeId and optional steps array"
        ));
    }
    Ok(root)
}

fn steps(root: &Value) -> impl Iterator<Item = (usize, &Value)> {
    root["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, step)| (i + 1, step))
}

fn user_text(step: &Value) -> Option<&str> {
    step.pointer("/userInput/userResponse")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty() && !is_junk(text))
}

fn row(path: &str, root: &Value) -> Row {
    let (line, opening) = steps(root)
        .find_map(|(i, step)| user_text(step).map(|text| (i as u64, clean(text, 110))))
        .unwrap_or_default();
    Row {
        id: id(Path::new(path))
            .unwrap_or_else(|| root["cascadeId"].as_str().unwrap())
            .into(),
        path: path.into(),
        hit: path.into(),
        agent: "agy".into(),
        title: opening.clone(),
        opening,
        line,
        mtime: mtime(path),
        sub: root
            .pointer("/agyReader/parentCascadeId")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.is_empty()),
        ..Default::default()
    }
}

pub fn session(path: &str) -> Result<Row, String> {
    Ok(row(path, &load(path)?))
}

fn message(session: &Row, index: usize, step: &Value, show: Show) -> Option<Message> {
    let (role, text) = if let Some(user) = user_text(step) {
        ("user", user.to_string())
    } else if let Some(response) = step.get("plannerResponse") {
        let mut text = response["response"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if show.think
            && let Some(thinking) = response["thinking"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
        {
            text.push_str(&format!(
                "\n{}",
                thinking
                    .lines()
                    .map(|line| format!("> {line}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if show.tools
            && let Some(calls) = response["toolCalls"].as_array()
        {
            for call in calls {
                if let Some(name) = call["name"].as_str() {
                    let args = call["argumentsJson"].as_str().unwrap_or_default();
                    text.push_str(&format!("\n- `{name}` {}", clean(args, 200)));
                }
            }
        }
        ("assistant", text)
    } else {
        return None;
    };
    let text = text.trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some(Message::new(
        session,
        step.pointer("/metadata/createdAt")
            .and_then(Value::as_str)
            .and_then(|s| s.parse().ok()),
        role.into(),
        text,
        Source {
            path: session.path.clone(),
            line: None,
            index: Some(index),
            record_id: step
                .pointer("/plannerResponse/messageId")
                .and_then(Value::as_str)
                .map(str::to_string),
            parts: Vec::new(),
        },
    ))
}

pub fn messages(path: &str, at: u64, show: Show) -> Result<Vec<Message>, String> {
    let root = load(path)?;
    let session = row(path, &root);
    Ok(steps(&root)
        .filter(|(i, _)| at == 0 || *i as u64 == at)
        .filter_map(|(i, step)| message(&session, i, step, show))
        .collect())
}

pub fn search_content(pattern: &regex::Regex, query: &str) -> Vec<Row> {
    paths()
        .into_iter()
        .filter_map(|path| {
            let path = path.to_str()?;
            let root = match load(path) {
                Ok(root) => root,
                Err(error) => {
                    eprintln!("asf: {error}");
                    return None;
                }
            };
            let mut row = row(path, &root);
            let hit = steps(&root)
                .filter_map(|(i, step)| {
                    message(
                        &row,
                        i,
                        step,
                        Show {
                            tools: true,
                            think: true,
                        },
                    )
                })
                .find(|message| pattern.is_match(&message.text))?;
            row.line = hit.source.index.unwrap() as u64;
            row.matched = window(&hit.text, query, 110);
            Some(row)
        })
        .collect()
}
