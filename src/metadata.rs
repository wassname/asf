//! Persistent metadata and FFF search; message bodies are not indexed. -- PI/OpenAI

use crate::record::{UUID, clean, find_value, is_junk, parse, role_of, texts};
use crate::sessions::{Row, session_id};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, Metadata};
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Stamp {
    size: u64,
    time: [u64; 2],
    file: [u64; 2],
}

impl Stamp {
    fn new(metadata: &Metadata) -> Self {
        let time = metadata
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap();
        Self {
            size: metadata.len(),
            time: [time.as_secs(), time.subsec_nanos() as u64],
            file: [metadata.dev(), metadata.ino()],
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    row: Row,
    stamp: Stamp,
    offset: u64,
    lines: u64,
    anchors: [u64; 2],
    titles: [String; 3],
    users: usize,
    header: bool,
    present: bool,
    partial: bool,
}

impl Entry {
    fn new(path: &Path, agent: &str, stamp: Stamp) -> Self {
        let path = path.to_string_lossy().into_owned();
        Self {
            row: Row {
                id: if agent == "pi" {
                    String::new()
                } else {
                    session_id(&path, agent)
                },
                agent: agent.to_string(),
                sub: path.contains("/subagents/"),
                hit: path.clone(),
                path,
                line: 1,
                ..Row::default()
            },
            stamp,
            offset: 0,
            lines: 0,
            anchors: [0; 2],
            titles: Default::default(),
            users: 0,
            header: false,
            present: false,
            partial: false,
        }
    }

    fn apply(&mut self, line: &[u8], number: u64) -> bool {
        let Ok(line) = std::str::from_utf8(line) else {
            return false;
        };
        let Some(record) = parse(line) else {
            return false;
        };
        let kind = record["type"].as_str().unwrap_or("");
        let agent = self.row.agent.as_str();
        let header = match agent {
            "pi" => kind == "session",
            "codex" => kind == "session_meta",
            "copilot" => kind == "session.start",
            _ => false,
        };
        if header && !self.header {
            self.header = true;
            self.present = true;
            self.row.cwd = find_value(&record, "cwd");
            if agent == "pi" {
                self.row.id = find_value(&record, "id");
                self.row.sub = UUID.find(&self.row.id).is_none();
            } else if agent == "codex" {
                self.row.sub = line.contains("\"subagent\"");
            }
        }
        if self.users < 4 && self.row.opening.is_empty() && role_of(&record) == "user" {
            self.present = true;
            self.users += 1;
            if self.row.cwd.is_empty() {
                self.row.cwd = find_value(&record, "cwd");
            }
            let said = texts(&record).join(" ");
            if !said.trim().is_empty() && !is_junk(&said) {
                self.row.opening = clean(&said, 110);
                self.row.line = number;
                if self.row.title.is_empty() {
                    self.row.title = self.row.opening.clone();
                }
            }
        }
        if agent == "pi" && kind == "session_info" {
            let name = find_value(&record, "name");
            if !name.is_empty() {
                self.row.title = clean(&name, 110);
                self.present = true;
            }
        }
        if agent == "claude" {
            let field = match kind {
                "ai-title" => Some((0, "aiTitle")),
                "custom-title" => Some((1, "customTitle")),
                "agent-name" => Some((2, "agentName")),
                _ => None,
            };
            if let Some((index, key)) = field {
                self.present = true;
                self.titles[index] = find_value(&record, key);
                self.row.title = self
                    .titles
                    .iter()
                    .rev()
                    .find(|name| !name.is_empty())
                    .map_or_else(|| self.row.opening.clone(), |name| clean(name, 110));
            }
        }
        true
    }

    fn needs_record(&self, prefix: &[u8]) -> bool {
        // Known JSONL formats put their record kind before the message body. -- PI/OpenAI
        static KIND: std::sync::LazyLock<regex::bytes::Regex> = std::sync::LazyLock::new(|| {
            regex::bytes::Regex::new(r#""type"\s*:\s*"([^"]+)""#).unwrap()
        });
        let Some(kind) = KIND.captures(prefix) else {
            return true;
        };
        match &kind[1] {
            b"session" | b"session_meta" | b"session.start" => !self.header,
            b"session_info" | b"ai-title" | b"custom-title" | b"agent-name" => true,
            b"user" | b"message" | b"response_item" => {
                self.users < 4 && self.row.opening.is_empty()
            }
            kind if kind.starts_with(b"user.") => self.users < 4 && self.row.opening.is_empty(),
            _ => false,
        }
    }
}

fn digest(bytes: &[u8]) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hash);
    hash.finish()
}

fn anchors(file: &mut File, offset: u64) -> [u64; 2] {
    let count = offset.min(4096) as usize;
    let mut bytes = vec![0; count];
    file.seek(SeekFrom::Start(0)).unwrap();
    file.read_exact(&mut bytes).unwrap();
    let first = digest(&bytes);
    file.seek(SeekFrom::Start(offset - count as u64)).unwrap();
    file.read_exact(&mut bytes).unwrap();
    [first, digest(&bytes)]
}

fn skip_record(reader: &mut impl BufRead) -> (u64, bool) {
    let mut skipped = 0;
    loop {
        let bytes = reader.fill_buf().unwrap();
        if bytes.is_empty() {
            return (skipped, false);
        }
        let newline = bytes.iter().position(|byte| *byte == b'\n');
        let count = newline.map_or(bytes.len(), |index| index + 1);
        reader.consume(count);
        skipped += count as u64;
        if newline.is_some() {
            return (skipped, true);
        }
    }
}

fn read_records(file: &mut File, entry: &mut Entry) {
    file.seek(SeekFrom::Start(entry.offset)).unwrap();
    let mut reader = BufReader::new(file.take(entry.stamp.size - entry.offset));
    while !reader.fill_buf().unwrap().is_empty() {
        if entry.partial || !entry.needs_record(reader.fill_buf().unwrap()) {
            let (count, terminated) = skip_record(&mut reader);
            entry.offset += count;
            entry.partial = !terminated;
            entry.lines += u64::from(terminated);
            continue;
        }
        let mut line = Vec::new();
        let count = reader.read_until(b'\n', &mut line).unwrap();
        if line.last() != Some(&b'\n') {
            // Keep incomplete JSON at its start; complete JSON may await its newline. -- PI/OpenAI
            if entry.apply(&line, entry.lines + 1) {
                entry.offset += count as u64;
                entry.partial = true;
            }
            break;
        }
        entry.lines += 1;
        entry.apply(&line, entry.lines);
        entry.offset += count as u64;
    }
}

pub struct MetadataIndex {
    root: PathBuf,
    entries: HashMap<String, Entry>,
    changed: bool,
}

impl MetadataIndex {
    pub fn load() -> Self {
        let root = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| crate::sessions::home().join(".cache"))
            .join("asf/metadata-v2");
        let entries = match std::fs::read(root.join("index.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .expect("invalid asf metadata index; remove the cache to rebuild it"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(error) => panic!("cannot read asf metadata index: {error}"),
        };
        Self {
            root,
            entries,
            changed: false,
        }
    }

    pub fn cached_row(&self, path: &Path) -> Option<&Row> {
        let entry = self.entries.get(path.to_str()?)?;
        (entry.present && entry.stamp == Stamp::new(&path.metadata().ok()?)).then_some(&entry.row)
    }

    pub fn cached_cwd(&self, path: &Path) -> Option<&str> {
        self.cached_row(path)
            .map(|row| row.cwd.as_str())
            .filter(|cwd| !cwd.is_empty())
    }

    pub fn refresh(&mut self, path: &Path, agent: &str) -> Vec<Row> {
        if agent == "agy" {
            return match crate::agy::session(&path.to_string_lossy()) {
                Ok(row) => vec![row],
                Err(error) => {
                    eprintln!("asf: {error}");
                    Vec::new()
                }
            };
        }
        let key = path.to_string_lossy().into_owned();
        let stamp = match path.metadata() {
            Ok(metadata) => Stamp::new(&metadata),
            Err(error) => {
                eprintln!("asf: {}: {error}", path.display());
                return Vec::new();
            }
        };
        if let Some(entry) = self.entries.get(&key)
            && entry.stamp == stamp
        {
            return if entry.present {
                vec![entry.row.clone()]
            } else {
                Vec::new()
            };
        }
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) => {
                eprintln!("asf: {}: {error}", path.display());
                return Vec::new();
            }
        };
        let standalone = ["gemini", "opencode"].contains(&agent);
        let mut entry = match self.entries.get(&key) {
            Some(entry)
                if !standalone
                    && entry.stamp.file == stamp.file
                    && stamp.size > entry.stamp.size
                    && anchors(&mut file, entry.offset) == entry.anchors =>
            {
                entry.clone()
            }
            _ => Entry::new(path, agent, stamp.clone()),
        };
        entry.stamp = stamp;
        if standalone {
            if let Some(row) = crate::sessions::standalone_metadata(path, agent) {
                entry.row = row;
                entry.row.id = session_id(&key, agent);
                entry.present = true;
            }
            entry.offset = entry.stamp.size;
        } else {
            read_records(&mut file, &mut entry);
        }
        entry.anchors = anchors(&mut file, entry.offset);
        entry.row.mtime = entry.stamp.time[0] as f64 + entry.stamp.time[1] as f64 * 1e-9;
        let rows = if entry.present {
            vec![entry.row.clone()]
        } else {
            Vec::new()
        };
        self.entries.insert(key, entry);
        self.changed = true;
        rows
    }

    pub fn exact_id(&mut self, id: &str, agent: Option<&str>, sub: bool) -> Vec<String> {
        let candidates: Vec<_> = self
            .entries
            .values()
            .filter(|entry| {
                entry.present
                    && entry.row.agent != "agy"
                    && entry.row.id == id
                    && !entry.row.path.contains("/subagents/")
                    && agent.is_none_or(|agent| entry.row.agent == agent)
                    && (sub || !entry.row.sub)
            })
            .map(|entry| (entry.row.path.clone(), entry.row.agent.clone()))
            .collect();
        candidates
            .into_iter()
            .filter(|(path, source)| {
                crate::sessions::agent_of(path) == *source && Path::new(path).exists()
            })
            .flat_map(|(path, source)| self.refresh(Path::new(&path), &source))
            .filter(|row| row.id == id && (sub || !row.sub))
            .map(|row| row.path)
            .collect()
    }

    fn save(&self) -> std::io::Result<()> {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.root)?;
        let temporary = self.root.join(format!("index.{}.tmp", std::process::id()));
        write_private(&temporary, &serde_json::to_vec(&self.entries)?)?;
        std::fs::rename(temporary, self.root.join("index.json"))
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?
        .write_all(bytes)
}

impl Drop for MetadataIndex {
    fn drop(&mut self) {
        if self.changed {
            self.save().expect("cannot save asf metadata index");
        }
    }
}
