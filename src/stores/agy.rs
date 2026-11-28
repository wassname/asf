//! Native Antigravity SQLite conversations, decoded read-only. -- PI/OpenAI

use crate::record::{Show, UUID, clean, is_junk, window};
use crate::sessions::{Row, home, mtime};
use crate::transcript::{Message, Source};
use jiff::Timestamp;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const CLI: &str = ".gemini/antigravity-cli/conversations";
pub const IDE: &str = ".gemini/antigravity/conversations";

pub fn id(path: &Path) -> Option<&str> {
    let id = path.file_name()?.to_str()?.strip_suffix(".db")?;
    (id.len() == 36 && UUID.is_match(id)).then_some(id)
}

pub fn paths() -> Vec<PathBuf> {
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
        .filter(|path| id(path).is_some())
        .collect()
}

pub fn modified(path: &str) -> f64 {
    let wal = format!("{path}-wal");
    if std::fs::metadata(&wal).is_ok_and(|meta| meta.len() > 0) {
        mtime(path).max(mtime(&wal))
    } else {
        mtime(path)
    }
}

#[derive(serde::Deserialize)]
struct Record {
    idx: i64,
    kind: u8,
    format: u64,
    payload: String,
}

fn load(path: &str, filter: &str) -> Result<(Row, Vec<Record>), String> {
    // The first result is trajectory metadata; subsequent results are chat steps in source order.
    let sql = format!(
        "SELECT -1 AS idx, 0 AS kind, 0 AS format,
        hex((SELECT data FROM trajectory_metadata_blob WHERE id = 'main')) AS payload
        UNION ALL SELECT idx, step_type, step_format, hex(step_payload) FROM steps
        WHERE {filter} ORDER BY idx;"
    );
    let out = Command::new("sqlite3")
        .args([
            "-init",
            "/dev/null",
            "-readonly",
            "-batch",
            "-cmd",
            ".timeout 1000",
            "-json",
            path,
            &sql,
        ])
        .output()
        .map_err(|err| format!("{path}: Antigravity needs sqlite3: {err}"))?;
    if !out.status.success() {
        return Err(format!(
            "{path}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let mut records: Vec<Record> = serde_json::from_slice(&out.stdout)
        .map_err(|err| format!("{path}: invalid SQLite output: {err}"))?;
    if records.first().is_none_or(|record| record.idx != -1) {
        return Err(format!("{path}: missing trajectory metadata row"));
    }
    let metadata = records.remove(0).decode()?;
    let metadata =
        Proto::parse(&metadata).map_err(|err| format!("{path}: trajectory metadata: {err}"))?;
    let cwd = metadata
        .bytes(7)
        .filter_map(file_uri)
        .next()
        .unwrap_or_default();
    let session = Row {
        id: id(Path::new(path))
            .ok_or_else(|| format!("{path}: expected an Antigravity UUID.db file"))?
            .into(),
        path: path.into(),
        hit: path.into(),
        agent: "agy".into(),
        cwd,
        mtime: modified(path),
        sub: !metadata.text(5).is_empty(),
        ..Default::default()
    };
    Ok((session, records))
}

fn file_uri(value: &[u8]) -> Option<String> {
    let path = value
        .strip_prefix(b"file://")
        .filter(|path| path.starts_with(b"/"))?;
    let mut decoded = Vec::with_capacity(path.len());
    let mut i = 0;
    while i < path.len() {
        if path[i] == b'%' {
            let pair = std::str::from_utf8(path.get(i + 1..i + 3)?).ok()?;
            decoded.push(u8::from_str_radix(pair, 16).ok()?);
            i += 3;
        } else {
            decoded.push(path[i]);
            i += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

pub fn session(path: &str) -> Result<Row, String> {
    collect(path, 0, Show::default(), true).map(|(row, _)| row)
}

pub fn conversation(path: &str, at: u64, show: Show) -> Result<(Row, Vec<Message>), String> {
    collect(path, at, show, false)
}

fn collect(path: &str, at: u64, show: Show, opening: bool) -> Result<(Row, Vec<Message>), String> {
    let filter = if opening {
        "step_type = 14".into()
    } else if at > 0 {
        format!("step_type IN (14,15) AND idx = {}", at - 1)
    } else {
        "step_type IN (14,15)".into()
    };
    let (mut row, records) = load(path, &filter)?;
    let mut messages = Vec::new();
    for record in records {
        let message = record
            .message(&row, show)
            .map_err(|err| format!("{path}: step {}: {err}", record.idx))?;
        let Some(message) = message else { continue };
        if message.role == "user" && row.opening.is_empty() {
            row.opening = clean(&message.text, 110);
            row.title = row.opening.clone();
            row.line = message.number();
            if opening {
                break;
            }
        }
        messages.push(message);
    }
    Ok((row, messages))
}

pub fn search_content(
    pattern: &regex::Regex,
    query: &str,
    in_cwd: Option<&dyn Fn(&str) -> bool>,
) -> Vec<Row> {
    paths()
        .into_iter()
        .filter_map(|path| {
            let path = path.to_str()?;
            if let Some(in_cwd) = in_cwd
                && !load(path, "0").is_ok_and(|(row, _)| in_cwd(&row.cwd))
            {
                return None;
            }
            let (mut row, messages) = match conversation(
                path,
                0,
                Show {
                    tools: true,
                    think: true,
                },
            ) {
                Ok(result) => result,
                Err(error) => {
                    eprintln!("asf: {error}");
                    return None;
                }
            };
            let hit = messages
                .iter()
                .find(|message| pattern.is_match(&message.text))?;
            row.line = hit.number();
            row.matched = window(&hit.text, query, 110);
            Some(row)
        })
        .collect()
}

impl Record {
    fn decode(&self) -> Result<Vec<u8>, String> {
        if self.format != 0 {
            return Err(format!(
                "unsupported Antigravity step format {}",
                self.format
            ));
        }
        (0..self.payload.len())
            .step_by(2)
            .map(|i| {
                self.payload
                    .get(i..i + 2)
                    .and_then(|s| u8::from_str_radix(s, 16).ok())
                    .ok_or_else(|| "invalid SQLite BLOB encoding".into())
            })
            .collect()
    }

    // SQLite format 0 and field numbers: agy-reader internal/completeness/sqlite.go and the 1.3.6 schema snapshot.
    fn message(&self, session: &Row, show: Show) -> Result<Option<Message>, String> {
        let bytes = self.decode()?;
        let step = Proto::parse(&bytes)?;
        let (key, role) = match self.kind {
            14 => (19, "user"),
            15 => (20, "assistant"),
            _ => unreachable!(),
        };
        let turn = Proto::parse(step.last_bytes(key).ok_or("chat payload is missing")?)?;
        let mut text;
        let record_id;
        if role == "user" {
            let input = [2, 1]
                .into_iter()
                .map(|key| turn.text(key))
                .find(|text| !text.trim().is_empty());
            let Some(input) = input.filter(|text| !is_junk(text)) else {
                return Ok(None);
            };
            text = input.to_owned();
            record_id = None;
        } else {
            text = turn.text(1).to_owned();
            let id = turn.text(6);
            record_id = (!id.is_empty()).then(|| id.to_owned());
            if show.think {
                for line in turn.text(3).lines() {
                    text.push_str(&format!("\n> {line}"));
                }
            }
            if show.tools {
                for call in turn.bytes(7) {
                    let call = Proto::parse(call)?;
                    text.push_str(&format!(
                        "\n- `{}` {}",
                        call.text(2),
                        clean(call.text(3), 200)
                    ));
                }
            }
        }
        if text.trim().is_empty() {
            return Ok(None);
        }
        let timestamp = if let Some(metadata) = step.last_bytes(5) {
            let metadata = Proto::parse(metadata)?;
            if let Some(time) = metadata.last_bytes(1) {
                let time = Proto::parse(time)?;
                Timestamp::new(time.number(1) as i64, time.number(2) as i32).ok()
            } else {
                None
            }
        } else {
            None
        };
        let index = usize::try_from(self.idx)
            .ok()
            .and_then(|i| i.checked_add(1))
            .ok_or("invalid step index")?;
        Ok(Some(Message::new(
            session,
            timestamp,
            role.into(),
            text.trim().into(),
            Source {
                path: session.path.clone(),
                line: None,
                index: Some(index),
                record_id,
                parts: Vec::new(),
            },
        )))
    }
}

// Minimal protobuf wire reader: skip fixed-width fields and expose only varints and byte strings.
#[derive(Clone, Copy)]
enum Field<'a> {
    Number(u64),
    Bytes(&'a [u8]),
}
struct Proto<'a>(Vec<(u64, Field<'a>)>);

impl<'a> Proto<'a> {
    fn parse(mut input: &'a [u8]) -> Result<Self, &'static str> {
        let mut fields = Vec::new();
        while !input.is_empty() {
            let key = varint(&mut input)?;
            let number = key >> 3;
            if number == 0 {
                return Err("invalid protobuf field number");
            }
            let field = match key & 7 {
                0 => Field::Number(varint(&mut input)?),
                2 => {
                    let len = usize::try_from(varint(&mut input)?)
                        .map_err(|_| "protobuf length overflow")?;
                    let (value, rest) = input
                        .split_at_checked(len)
                        .ok_or("truncated protobuf field")?;
                    input = rest;
                    Field::Bytes(value)
                }
                wire @ (1 | 5) => {
                    input = input
                        .get(if wire == 1 { 8 } else { 4 }..)
                        .ok_or("truncated fixed-width protobuf field")?;
                    continue;
                }
                _ => return Err("unsupported protobuf wire type"),
            };
            fields.push((number, field));
        }
        Ok(Self(fields))
    }

    fn bytes(&self, key: u64) -> impl DoubleEndedIterator<Item = &'a [u8]> + '_ {
        self.0
            .iter()
            .filter_map(move |&(number, field)| match field {
                Field::Bytes(value) if number == key => Some(value),
                _ => None,
            })
    }

    fn last_bytes(&self, key: u64) -> Option<&'a [u8]> {
        self.bytes(key).next_back()
    }
    fn text(&self, key: u64) -> &'a str {
        self.last_bytes(key)
            .and_then(|s| std::str::from_utf8(s).ok())
            .unwrap_or_default()
    }
    fn number(&self, key: u64) -> u64 {
        self.0
            .iter()
            .rev()
            .find_map(|&(number, field)| match field {
                Field::Number(value) if number == key => Some(value),
                _ => None,
            })
            .unwrap_or_default()
    }
}

fn varint(input: &mut &[u8]) -> Result<u64, &'static str> {
    let mut result = 0;
    for shift in (0..64).step_by(7) {
        let (&byte, rest) = input.split_first().ok_or("truncated protobuf varint")?;
        *input = rest;
        if shift == 63 && byte > 1 {
            return Err("protobuf varint overflow");
        }
        result |= u64::from(byte & 127) << shift;
        if byte < 128 {
            return Ok(result);
        }
    }
    Err("protobuf varint overflow")
}
