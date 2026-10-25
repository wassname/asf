//! Normalized messages, source locations, and rendering. -- PI/OpenAI

use crate::record::{Show, cut, extras, is_junk, parse, role_of, texts};
use crate::sessions::{self, Row};
use jiff::Timestamp;
use serde::{Serialize, Serializer};
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Serialize)]
pub struct Source {
    pub path: String,
    pub line: Option<u64>,
    pub index: Option<usize>,
    pub record_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<String>,
}

#[derive(Serialize)]
pub struct Message {
    pub session_id: String,
    pub agent: String,
    pub cwd: Option<String>,
    #[serde(serialize_with = "serialize_timestamp")]
    pub timestamp: Option<Timestamp>,
    pub role: String,
    pub text: String,
    pub source: Source,
}

fn serialize_timestamp<S: Serializer>(
    time: &Option<Timestamp>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    time.map(|time| time.to_string()).serialize(serializer)
}

impl Message {
    pub fn new(
        session: &Row,
        timestamp: Option<Timestamp>,
        role: String,
        text: String,
        source: Source,
    ) -> Self {
        Self {
            session_id: session.id.clone(),
            agent: session.agent.clone(),
            cwd: (!session.cwd.is_empty()).then(|| session.cwd.clone()),
            timestamp,
            role,
            text,
            source,
        }
    }

    pub fn markdown(&self) -> String {
        format!("## {}\n{}", self.role, self.text)
    }
}

fn record_id(entry: &Value, agent: &str) -> Option<String> {
    let value = match agent {
        "claude" => entry.get("uuid"),
        "codex" => entry.pointer("/payload/id"),
        "gemini" => entry.get("messageId").or_else(|| entry.get("id")),
        _ => entry.get("id"),
    }?;
    match value {
        Value::String(id) => Some(id.clone()),
        Value::Number(id) => Some(id.to_string()),
        _ => None,
    }
}

pub fn unix_seconds(value: &Value) -> Option<Timestamp> {
    let seconds = value.as_f64()?;
    let whole = seconds.floor();
    let nanos = ((seconds - whole) * 1_000_000_000.0).round() as i32;
    Timestamp::new(whole as i64, nanos).ok()
}

fn message_time(entry: &Value, agent: &str) -> Option<Timestamp> {
    if agent == "opencode" {
        return Timestamp::from_millisecond(entry.pointer("/time/created")?.as_i64()?).ok();
    }
    if let Some(time) = entry.get("timestamp") {
        return time.as_str()?.parse().ok();
    }
    if agent == "pi" {
        return Timestamp::from_millisecond(entry.pointer("/message/timestamp")?.as_i64()?).ok();
    }
    None
}

fn speech(entry: &Value, verbatim: bool, show: Show) -> Option<(String, String)> {
    let result = entry.get("toolUseResult").is_some() && !show.tools;
    if !verbatim && (result || entry.get("isMeta") == Some(&Value::Bool(true))) {
        return None;
    }
    let calls = extras(entry, show);
    let said: Vec<String> = texts(entry)
        .into_iter()
        .filter(|text| !text.trim().is_empty())
        .filter(|text| verbatim || !is_junk(text))
        .chain(calls.iter().cloned())
        .collect();
    let mut body = said.join("\n");
    if verbatim && body.is_empty() {
        body = cut(&serde_json::to_string_pretty(entry).unwrap(), 4000);
    }
    let spoke = role_of(entry);
    let turn =
        ["user", "assistant"].contains(&spoke.as_str()) && entry.get("toolUseResult").is_none();
    let role = if turn { spoke } else { "tool".to_string() };
    if body.is_empty() || !(verbatim || turn || !calls.is_empty()) {
        return None;
    }
    Some((role, body))
}

fn normalize(
    entry: &Value,
    session: &Row,
    source: Source,
    verbatim: bool,
    show: Show,
) -> Option<Message> {
    let (role, text) = speech(entry, verbatim, show)?;
    let mut message = Message::new(
        session,
        message_time(entry, &session.agent),
        role,
        text,
        source,
    );
    if session.agent == "gemini"
        && let Some(id) = entry.get("sessionId").and_then(Value::as_str)
    {
        message.session_id = id.to_string();
    }
    Some(message)
}

fn context(path: &str) -> Row {
    let agent = sessions::agent_of(path);
    let mut session = sessions::indexed_row(path).unwrap_or_else(|| Row {
        agent: agent.clone(),
        path: path.to_string(),
        ..Row::default()
    });
    if session.id.is_empty() {
        session.id = sessions::session_id(path, &agent);
    }
    session
}

fn array_messages(entries: &[Value], session: &Row, show: Show) -> Vec<Message> {
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            normalize(
                entry,
                session,
                Source {
                    path: session.path.clone(),
                    line: None,
                    index: Some(index + 1),
                    record_id: record_id(entry, &session.agent),
                    parts: Vec::new(),
                },
                false,
                show,
            )
        })
        .collect()
}

pub fn messages(path: &str, at: u64, show: Show) -> Result<Vec<Message>, String> {
    let session = context(path);
    if session.agent == "hermes" {
        return Ok(crate::hermes::messages(&session, show));
    }
    if session.agent == "opencode" {
        let mut messages = Vec::new();
        for (file, parts) in sessions::opencode_messages(path) {
            let entry = sessions::read_json(&file)
                .ok_or_else(|| format!("cannot read OpenCode message {}", file.display()))?;
            let said: Vec<String> = parts
                .iter()
                .filter_map(|part| sessions::read_json(part))
                .flat_map(|part| texts(&part).into_iter().chain(extras(&part, show)))
                .filter(|text| !text.trim().is_empty())
                .collect();
            if !said.is_empty() {
                messages.push(Message::new(
                    &session,
                    message_time(&entry, "opencode"),
                    entry["role"].as_str().unwrap_or("?").to_string(),
                    said.join("\n"),
                    Source {
                        path: file.to_string_lossy().into_owned(),
                        line: None,
                        index: None,
                        record_id: record_id(&entry, "opencode"),
                        parts: parts
                            .iter()
                            .map(|p| p.to_string_lossy().into_owned())
                            .collect(),
                    },
                ));
            }
        }
        return Ok(messages);
    }

    let file = File::open(path).map_err(|error| format!("{path}: {error}"))?;
    let mut reader = BufReader::new(file);
    let array = reader
        .fill_buf()
        .map_err(|error| error.to_string())?
        .iter()
        .find(|byte| !byte.is_ascii_whitespace())
        == Some(&b'[');
    if array
        || (session.agent == "gemini"
            && Path::new(path).extension().is_some_and(|ext| ext == "json"))
    {
        let root: Value =
            serde_json::from_reader(reader).map_err(|error| format!("{path}: {error}"))?;
        let mut session = session;
        if let Some(id) = root.get("sessionId").and_then(Value::as_str) {
            session.id = id.to_string();
        }
        let entries = root
            .as_array()
            .or_else(|| root.get("messages").and_then(Value::as_array))
            .ok_or_else(|| format!("{path}: expected a message array"))?;
        return Ok(array_messages(entries, &session, show));
    }

    let mut messages: Vec<Message> = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let number = index as u64 + 1;
        if at != 0 && at != number {
            continue;
        }
        let line = line.map_err(|error| format!("{path}:{number}: {error}"))?;
        let Some(entry) = parse(&line) else { continue };
        let source = Source {
            path: path.to_string(),
            line: Some(number),
            index: None,
            record_id: record_id(&entry, &session.agent),
            parts: Vec::new(),
        };
        let Some(message) = normalize(&entry, &session, source, at != 0, show) else {
            continue;
        };
        messages.push(message);
    }
    Ok(messages)
}

/// Collapse adjacent resend copies for Markdown, never JSONL records. -- PI/OpenAI
pub fn deduplicate(messages: Vec<Message>) -> Vec<Message> {
    let mut kept: Vec<Message> = Vec::new();
    for message in messages {
        if message.source.line.is_some() {
            match kept.last() {
                Some(last) if last.role == message.role && message.text.starts_with(&last.text) => {
                    kept.pop();
                }
                Some(last) if last.role == message.role && last.text.starts_with(&message.text) => {
                    continue;
                }
                _ => {}
            }
        }
        kept.push(message);
    }
    kept
}

#[derive(Default)]
pub struct ReadOptions<'a> {
    pub head: usize,
    pub tail: usize,
    pub at: u64,
    pub show: Show,
    pub role: Option<&'a str>,
    pub since: Option<Timestamp>,
    pub until: Option<Timestamp>,
    pub jsonl: bool,
}

pub fn read(path: &str, options: ReadOptions<'_>) -> Result<String, String> {
    let mut unknown = 0;
    let dated = options.since.is_some() || options.until.is_some();
    let messages: Vec<Message> = messages(path, options.at, options.show)?
        .into_iter()
        .filter(|message| {
            if !dated {
                return true;
            }
            let Some(time) = message.timestamp else {
                if options.role.is_none_or(|role| message.role == role) {
                    unknown += 1;
                }
                return false;
            };
            options.since.is_none_or(|since| time >= since)
                && options.until.is_none_or(|until| time <= until)
        })
        .collect();
    if unknown > 0 {
        eprintln!("asf: excluded {unknown} messages with unknown timestamps from {path}");
    }
    let messages = if options.jsonl {
        messages
    } else {
        deduplicate(messages)
    };
    let messages: Vec<Message> = messages
        .into_iter()
        .filter(|message| options.role.is_none_or(|role| message.role == role))
        .collect();
    let cut = options.head.saturating_add(options.tail);
    let trimmed = cut != 0 && messages.len() > cut;
    let mut output = Vec::new();
    for (index, message) in messages.iter().enumerate() {
        if trimmed && index >= options.head && index < messages.len() - options.tail {
            if !options.jsonl && index == options.head {
                output.push(format!(
                    "---\n\n... {} messages omitted ...\n\n---",
                    messages.len() - cut
                ));
            }
            continue;
        }
        output.push(if options.jsonl {
            serde_json::to_string(message).unwrap()
        } else {
            message.markdown()
        });
    }
    Ok(output.join(if options.jsonl { "\n" } else { "\n\n" }))
}

pub fn time_bound(value: &str, now: Timestamp) -> Result<Timestamp, String> {
    if value == "now" {
        return Ok(now);
    }
    if let Ok(time) = value.parse::<Timestamp>() {
        return Ok(time);
    }
    if let Ok(date) = value.parse::<jiff::civil::Date>() {
        return date
            .to_zoned(jiff::tz::TimeZone::system())
            .map(|time| time.timestamp())
            .map_err(|error| error.to_string());
    }
    let split = value
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        "w" => 604800,
        _ => 0,
    };
    if multiplier != 0
        && let Ok(number) = number.parse::<u64>()
        && let Some(seconds) = number.checked_mul(multiplier)
    {
        return now
            .checked_sub(std::time::Duration::from_secs(seconds))
            .map_err(|error| error.to_string());
    }
    Err(format!(
        "invalid time {value:?}; use RFC3339, YYYY-MM-DD (local midnight), now, or a duration such as 24h"
    ))
}
