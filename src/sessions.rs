//! Where the sessions live, how each agent names one, and how to read one back.

use crate::record::*;
use crate::scan::{self, Hits, JSONISH, Scan};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Every session store on this machine. Add an agent by adding a line. Content search works
/// on any of them; names need one line in NAME_PATTERNS or in load_sessions.
/// hermes is absent on purpose: it keeps its sessions in ~/.hermes/state.db, a sqlite file.
pub const SOURCES: [(&str, &str); 6] = [
    ("claude", ".claude/projects"),
    ("codex", ".codex/sessions"),
    ("pi", ".pi/agent/sessions"),
    ("gemini", ".gemini/tmp"),
    ("opencode", ".local/share/opencode/storage"),
    ("copilot", ".copilot/session-state"),
];

/// how to reopen a session, by agent. gemini is absent: its --resume takes `latest` or a
/// list index, never an id.
const RESUME: [(&str, &str); 5] = [
    ("claude", "claude --resume {sid}"),
    ("codex", "codex resume {sid}"),
    ("pi", "pi --session {sid}"),
    ("opencode", "opencode --session {sid}"),
    ("copilot", "copilot --resume={sid}"),
];

/// what claude writes when you /rename a session
const RENAMED: &str = r#""type":"custom-title""#;

/// (first user message, session header) per agent. The header carries the working directory.
const NAME_PATTERNS: [(&str, &str, &str); 5] = [
    ("claude", r#""type":"user""#, ""),
    ("codex", r#""role":"user""#, r#""type":"session_meta""#),
    ("pi", r#""role":"user""#, r#""type":"session""#),
    ("copilot", r#""type": ?"user\."#, r#""type": ?"session\.start""#),
    ("gemini", r#""type":"user""#, r#""projectHash""#),
];

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").expect("HOME is not set"))
}

pub fn store(agent: &str) -> PathBuf {
    let (_, tail) = SOURCES.iter().find(|(a, _)| *a == agent).expect("no such agent");
    home().join(tail)
}

#[derive(Clone, Default)]
pub struct Row {
    pub path: String,
    pub hit: String,
    pub agent: String,
    pub cwd: String,
    pub title: String,
    pub matched: String,
    /// the first thing you said, kept even when a name replaces it in the title
    pub opening: String,
    pub raw: String,
    pub mtime: f64,
    pub line: u64,
    /// a run the agent started for itself, which you cannot resume
    pub sub: bool,
}

/// What one scan learned about a session.
#[derive(Default)]
struct Found {
    cwd: String,
    title: String,
    line: u64,
    /// the name the agent keeps for itself beats the first message
    force: bool,
    sub: bool,
}

/// Rows in the order they were first seen, which breaks ties after sorting by mtime.
#[derive(Default)]
pub struct Rows {
    order: Vec<String>,
    by_path: HashMap<String, Row>,
}

impl Rows {
    fn touch(&mut self, path: &str) -> &mut Row {
        if !self.by_path.contains_key(path) {
            self.order.push(path.to_string());
            self.by_path.insert(
                path.to_string(),
                Row {
                    path: path.to_string(),
                    hit: path.to_string(),
                    agent: agent_of(path),
                    line: 1,
                    // claude keeps its subagent logs in a directory of their own
                    sub: path.contains("/subagents/"),
                    ..Row::default()
                },
            );
        }
        self.by_path.get_mut(path).unwrap()
    }

    pub fn into_vec(self) -> Vec<Row> {
        let mut by_path = self.by_path;
        self.order
            .into_iter()
            .filter_map(|p| by_path.remove(&p))
            .collect()
    }
}

pub fn agent_of(path: &str) -> String {
    let home = home();
    for (agent, tail) in SOURCES {
        if path.starts_with(&home.join(tail).to_string_lossy().to_string()) {
            return agent.to_string();
        }
    }
    "?".to_string()
}

/// The id its own agent wants back. Claude subagent logs resume their parent.
pub fn session_id(path: &str, agent: &str) -> String {
    let p = Path::new(path);
    let stem = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let dir = |up: &Path| up.file_name().unwrap_or_default().to_string_lossy().to_string();
    if p.parent().and_then(Path::file_name).is_some_and(|n| n == "subagents")
        && let Some(parent) = p.parent().and_then(Path::parent) {
            return dir(parent);
        }
    match agent {
        "opencode" => stem,
        // the id is inside the record: a pi filename truncates it at an underscore, and for
        // 8 of 1216 sessions here the filename uuid is not the one pi answers to
        "pi" => first_record(path).map_or(stem, |e| find_value(&e, "id")),
        // copilot names the session directory, not the events.jsonl inside it
        "copilot" => p.parent().map_or(stem, dir),
        _ => match UUID.find(&stem) {
            Some(found) => found.as_str().to_string(),
            None => stem,
        },
    }
}

fn first_record(path: &str) -> Option<Value> {
    parse(std::io::BufRead::lines(std::io::BufReader::new(std::fs::File::open(path).ok()?)).next()?.ok()?.as_str())
}

pub fn resume_command(row: &Row) -> String {
    let Some((_, template)) = RESUME.iter().find(|(a, _)| *a == row.agent) else {
        return format!("# {} has no resume command; transcript: {}", row.agent, row.path);
    };
    let cmd = template.replace("{sid}", &session_id(&row.path, &row.agent));
    if row.cwd.is_empty() {
        return cmd;
    }
    if !Path::new(&row.cwd).exists() {
        // codex resumes by id from anywhere; claude, pi and opencode scope the lookup to the
        // directory, so for those this command is the proof that the session is stranded
        return format!("# the directory it ran in is gone: {}\n{cmd}", row.cwd);
    }
    format!("cd '{}' && {cmd}", row.cwd)
}

/// The resume command for one transcript path, read fresh. The picker needs this: after a
/// reload its rows are new sessions the caller never scanned.
pub fn resume_for_path(hit: &str) -> String {
    let path = session_of(hit);
    let agent = agent_of(&path);
    let mut row = Row { path: path.clone(), agent: agent.clone(), ..Row::default() };
    name_sessions(&[(agent, vec![PathBuf::from(&path)])], |_, found| {
        if row.cwd.is_empty() {
            row.cwd = found.cwd;
        }
    });
    resume_command(&row)
}

/// An opencode hit is one shard; every shard names the session it belongs to.
fn session_of(hit: &str) -> String {
    let stem = Path::new(hit).file_stem().unwrap_or_default().to_string_lossy().to_string();
    if agent_of(hit) != "opencode" || stem.starts_with("ses_") {
        return hit.to_string();
    }
    let id = read_json(Path::new(hit)).map_or(String::new(), |e| find_value(&e, "sessionID"));
    opencode_sessions().get(&id).cloned().unwrap_or_else(|| hit.to_string())
}

/// (line number, text) of the first message that is not a harness preamble.
fn first_real(hits: &[scan::Hit]) -> (u64, String) {
    for hit in hits {
        let Some(entry) = parse(&hit.text) else { continue };
        let said = texts(&entry).join(" ");
        if !said.trim().is_empty() && !is_junk(&said) {
            return (hit.line, said);
        }
    }
    (1, String::new())
}

fn read_json(path: &Path) -> Option<Value> {
    parse(&std::fs::read_to_string(path).ok()?)
}

/// Fill in name and working directory. Two searches per agent, over whole stores or over a
/// handful of files, which is why content mode can afford to call it too.
fn name_sessions<F>(stores: &[(String, Vec<PathBuf>)], mut add: F)
where
    F: FnMut(&str, Found),
{
    let names = |pattern: &str, paths: &[PathBuf], max_count: usize| -> Hits {
        scan::search(
            pattern,
            paths,
            &Scan { literal: false, icase: false, globs: &JSONISH, max_count },
        )
    };
    for (agent, paths) in stores {
        let found = NAME_PATTERNS.iter().find(|(a, _, _)| a == agent);
        if let Some((_, _, header)) = found.filter(|(_, _, h)| !h.is_empty()) {
            for (path, hits) in names(header, paths, 1) {
                let cwd = parse(&hits[0].text).map_or(String::new(), |e| find_value(&e, "cwd"));
                // codex records a run it started for itself like any other session
                let sub = hits[0].text.contains(r#""subagent""#);
                add(&path, Found { cwd, sub, ..Found::default() });
            }
        }
        if let Some((_, pattern, _)) = found.filter(|(_, p, _)| !p.is_empty()) {
            for (path, hits) in names(pattern, paths, 4) {
                let (line, said) = first_real(&hits);
                let cwd = parse(&hits[0].text).map_or(String::new(), |e| find_value(&e, "cwd"));
                add(&path, Found { cwd, title: clean(&said, 110), line, ..Found::default() });
            }
        }
        if agent == "claude" {
            // the name claude's own UI shows beats the first message
            for (path, hits) in names(r#""type":"ai-title""#, paths, 1) {
                let title = parse(&hits[0].text)
                    .map_or(String::new(), |e| find_value(&e, "aiTitle"));
                add(&path, Found { title: clean(&title, 110), force: true, ..Found::default() });
            }
            // /rename beats both, and repeats the record, so take the last one in the file.
            // Two passes: find the few files that were renamed, then read those in full.
            let renamed: Vec<PathBuf> =
                names(RENAMED, paths, 1).keys().map(PathBuf::from).collect();
            for (path, hits) in names(RENAMED, &renamed, usize::MAX) {
                let title = parse(&hits[hits.len() - 1].text)
                    .map_or(String::new(), |e| find_value(&e, "customTitle"));
                add(&path, Found { title: clean(&title, 110), force: true, ..Found::default() });
            }
        }
        if agent == "opencode" {
            // its metadata file already holds a title, and names its parent if it has one
            for path in paths {
                let meta = read_json(path).unwrap_or(Value::Null);
                let cwd = meta.get("directory").and_then(Value::as_str).unwrap_or("");
                let title = meta.get("title").and_then(Value::as_str).unwrap_or("");
                add(
                    &path.to_string_lossy(),
                    Found {
                        cwd: cwd.to_string(),
                        title: clean(title, 110),
                        sub: meta.get("parentID").is_some(),
                        ..Found::default()
                    },
                );
            }
        }
        if agent == "gemini" {
            // one logs.json of prompts per project, no per-session split
            for path in paths.iter().filter(|p| p.ends_with("logs.json")) {
                let Some(Value::Array(log)) = read_json(path) else { continue };
                let first = log
                    .iter()
                    .filter_map(|e| e.get("message").and_then(Value::as_str))
                    .find(|m| !m.is_empty())
                    .unwrap_or("");
                let project = path.parent().and_then(Path::file_name).unwrap_or_default();
                add(
                    &path.to_string_lossy(),
                    Found {
                        cwd: project.to_string_lossy().into_owned(),
                        title: clean(first, 110),
                        ..Found::default()
                    },
                );
            }
        }
    }
}

fn stores_for_names() -> Vec<(String, Vec<PathBuf>)> {
    let mut stores: Vec<(String, Vec<PathBuf>)> = NAME_PATTERNS
        .iter()
        .map(|(agent, _, _)| (agent.to_string(), vec![store(agent)]))
        .collect();
    let gemini = stores.iter_mut().find(|(a, _)| a == "gemini").unwrap();
    let tmp = store("gemini");
    if let Ok(entries) = std::fs::read_dir(&tmp) {
        for entry in entries.filter_map(Result::ok) {
            let log = entry.path().join("logs.json");
            if log.exists() {
                gemini.1.push(log);
            }
        }
    }
    stores.push((
        "opencode".to_string(),
        scan::files_under(&store("opencode").join("session"), "ses_"),
    ));
    stores
}

/// The name you typed, where the transcript itself does not hold it. codex keeps a thread
/// name in its own index; a pi session you started with `--session-id <name>` is named by its
/// own file, and only a generated session gets a uuid there.
fn typed_names(rows: &mut [Row]) {
    for row in rows.iter_mut().filter(|r| r.agent == "pi") {
        let stem = Path::new(&row.path).file_stem().unwrap_or_default().to_string_lossy();
        let Some((_, id)) = stem.split_once('_') else { continue };
        if UUID.find(id).is_none() {
            row.title = id.to_string();
        }
    }
    if !rows.iter().any(|r| r.agent == "codex") {
        return;
    }
    let index = std::fs::read_to_string(home().join(".codex/session_index.jsonl")).unwrap_or_default();
    let names: HashMap<String, String> = index
        .lines()
        .filter_map(parse)
        .map(|e| (find_value(&e, "id"), find_value(&e, "thread_name")))
        .filter(|(id, name)| !id.is_empty() && !name.is_empty())
        .collect();
    for row in rows.iter_mut().filter(|r| r.agent == "codex") {
        if let Some(name) = names.get(&session_id(&row.path, "codex")) {
            row.title = clean(name, 110);
        }
    }
}

/// One row per session: agent, path, mtime, cwd, title.
pub fn load_sessions() -> Vec<Row> {
    let mut rows = Rows::default();
    name_sessions(&stores_for_names(), |path, found| {
        let row = rows.touch(path);
        if row.cwd.is_empty() {
            row.cwd = found.cwd;
        }
        if found.line != 0 {
            // only the first-message pass reports a line, and that message is the opening
            row.line = found.line;
            row.opening = found.title.clone();
        }
        if (found.force && !found.title.is_empty()) || row.title.is_empty() {
            row.title = found.title;
        }
        row.sub |= found.sub;
    });
    let mut rows = rows.into_vec();
    for row in &mut rows {
        if row.agent == "gemini" && row.cwd.is_empty() {
            // ~/.gemini/tmp/<project>/chats/x.jsonl
            let parts: Vec<_> = Path::new(&row.path).components().collect();
            if parts.len() >= 3 {
                row.cwd = parts[parts.len() - 3].as_os_str().to_string_lossy().to_string();
            }
        }
        row.mtime = mtime(&row.path);
    }
    typed_names(&mut rows);
    rows
}

pub fn day(mtime: f64, format: &str) -> String {
    jiff::Timestamp::from_second(mtime as i64)
        .expect("mtime out of range")
        .to_zoned(jiff::tz::TimeZone::system())
        .strftime(format)
        .to_string()
}

pub fn mtime(path: &str) -> f64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0.0, |d| d.as_secs_f64())
}

/// {session id: its metadata file}. opencode shards a session over thousands of files, and
/// every shard names its own sessionID.
fn opencode_sessions() -> HashMap<String, String> {
    scan::files_under(&store("opencode").join("session"), "ses_")
        .into_iter()
        .map(|p| {
            let stem = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
            (stem, p.to_string_lossy().to_string())
        })
        .collect()
}

/// Search every store. One row per session, at its first hit.
pub fn search_content(query: &str, regex: bool) -> Vec<Row> {
    let roots: Vec<PathBuf> = SOURCES.iter().map(|(a, _)| store(a)).collect();
    let scan = |paths: &[PathBuf], max_count: usize| -> Hits {
        scan::search(
            query,
            paths,
            &Scan { literal: !regex, icase: true, globs: &scan::CONTENT_GLOBS, max_count },
        )
    };
    let mut hits = scan(&roots, 1);

    // where the only hit is harness-injected text, look deeper in those files alone
    let noisy: Vec<PathBuf> = hits
        .iter()
        .filter(|(_, h)| injected(&h[0].text, query))
        .map(|(p, _)| PathBuf::from(p))
        .collect();
    hits.extend(scan(&noisy, 6));

    // an opencode hit is one shard of a session; one more search tells us which session
    let shards: Vec<PathBuf> = hits
        .keys()
        .filter(|p| p.contains("/opencode/"))
        .map(PathBuf::from)
        .collect();
    let mut owner: HashMap<String, String> = HashMap::new();
    if !shards.is_empty() {
        let sessions = opencode_sessions();
        let found = scan::search(
            r#""sessionID""#,
            &shards,
            &Scan { literal: false, icase: false, globs: &[], max_count: 1 },
        );
        for (path, hits) in found {
            let session = SES
                .find(&hits[0].text)
                .and_then(|m| sessions.get(m.as_str()).cloned())
                .unwrap_or_else(|| path.clone());
            owner.insert(path, session);
        }
    }

    let mut rows = Rows::default();
    for (hit_path, hits) in &hits {
        // every hit in this file may be harness-injected text, not conversation
        let Some(real) = hits.iter().find(|h| !injected(&h.text, query)) else { continue };
        let path = owner.get(hit_path).unwrap_or(hit_path).clone();
        if rows.by_path.contains_key(&path) {
            continue;
        }
        let row = rows.touch(&path);
        row.hit = hit_path.clone();
        row.raw = real.text.clone();
        row.line = real.line;
        row.mtime = mtime(hit_path);
    }
    rows.into_vec()
}

/// Fill in the match text, the session name and the working directory. Only for the rows you
/// will show: a common word matches thousands of files, and this reads them.
pub fn hydrate(rows: &mut [Row], query: &str) {
    for row in rows.iter_mut() {
        if row.raw.is_empty() || !row.matched.is_empty() {
            continue;
        }
        let said = parse(&row.raw).map_or(String::new(), |e| texts(&e).join(" "));
        // a match inside a tool call is not in the speech, so fall back to the raw line
        let spoken = if query.is_empty() {
            !said.is_empty()
        } else {
            regex::Regex::new(&format!("(?i){}", regex::escape(query)))
                .is_ok_and(|re| re.is_match(&said))
        };
        row.matched = window(if spoken { &said } else { &row.raw }, query, 110);
    }

    let unnamed: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.title.is_empty())
        .map(|(i, _)| i)
        .collect();
    if unnamed.is_empty() {
        return;
    }
    let mut by_agent: Vec<(String, Vec<PathBuf>)> = Vec::new();
    for &i in &unnamed {
        let agent = rows[i].agent.clone();
        match by_agent.iter_mut().find(|(a, _)| *a == agent) {
            Some((_, paths)) => paths.push(PathBuf::from(&rows[i].path)),
            None => by_agent.push((agent, vec![PathBuf::from(&rows[i].path)])),
        }
    }
    let mut named: HashMap<String, (String, String)> = HashMap::new();
    name_sessions(&by_agent, |path, found| {
        let entry = named.entry(path.to_string()).or_default();
        if entry.0.is_empty() {
            entry.0 = found.cwd;
        }
        if (found.force && !found.title.is_empty()) || entry.1.is_empty() {
            entry.1 = found.title;
        }
    });
    for row in rows.iter_mut() {
        let Some((cwd, title)) = named.get(&row.path) else { continue };
        if row.cwd.is_empty() {
            row.cwd = cwd.clone();
        }
        if row.title.is_empty() {
            row.title = title.clone();
        }
    }
    typed_names(rows);
}

fn opencode_messages(session_json: &str) -> Vec<(PathBuf, Vec<PathBuf>)> {
    let root = store("opencode");
    let session = Path::new(session_json).file_stem().unwrap_or_default();
    let mut messages = scan::files_under(&root.join("message").join(session), "msg_");
    messages.sort();
    messages
        .into_iter()
        .map(|message| {
            let stem = message.file_stem().unwrap_or_default();
            let mut parts = scan::files_under(&root.join("part").join(stem), "prt_");
            parts.sort();
            (message, parts)
        })
        .collect()
}

/// opencode splits a session over storage/message/<ses>/ and storage/part/<msg>/.
fn opencode_transcript(session_json: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    for (message, parts) in opencode_messages(session_json) {
        let meta = read_json(&message).unwrap_or(Value::Null);
        let role = meta.get("role").and_then(Value::as_str).unwrap_or("?");
        let said: Vec<String> = parts
            .iter()
            .filter_map(|p| read_json(p))
            .flat_map(|p| texts(&p))
            .filter(|t| !t.trim().is_empty())
            .collect();
        if !said.is_empty() {
            blocks.push(format!("--- {role} ---\n{}", said.join("\n")));
        }
    }
    blocks
}

/// The transcript as text, one block per message. Works for every format here.
///
/// `at` prints only the record on that line, which is what the picker previews. Otherwise
/// only the conversation: tool calls, their results and the harness preambles are dropped.
pub fn read(path: &str, head: usize, tail: usize, at: u64) -> String {
    let blocks = blocks(path, at);
    ends(&blocks, head, tail).join("\n\n")
}

/// Mark the codex sessions an agent ran for itself. Name mode learns this while it reads the
/// headers; content mode never reads them, so it asks here.
pub fn mark_subagents(rows: &mut [Row]) {
    let codex: Vec<PathBuf> = rows
        .iter()
        .filter(|r| r.agent == "codex")
        .map(|r| PathBuf::from(&r.path))
        .collect();
    let sub = scan::search(
        r#""type":"session_meta"[^\n]*"subagent""#,
        &codex,
        &Scan { literal: false, icase: false, globs: &[], max_count: 1 },
    );
    for row in rows.iter_mut() {
        row.sub |= sub.contains_key(&row.path);
    }
}

/// Where it ran, the files it named, the record that matched, then its first and last words.
pub fn preview(path: &str, at: u64) -> String {
    let agent = agent_of(path);
    let mut cwd = String::new();
    let mut title = String::new();
    name_sessions(&[(agent.clone(), vec![PathBuf::from(path)])], |_, found| {
        if cwd.is_empty() {
            cwd = found.cwd;
        }
        if (found.force && !found.title.is_empty()) || title.is_empty() {
            title = found.title;
        }
    });

    let files = files_named(path);
    let label = |name: &str, value: String| format!("\x1b[2m{name:7}\x1b[0m{value}");
    let mut out = vec![
        label("client", agent.clone()),
        label("date", day(mtime(path), "%Y-%m-%d %H:%M")),
        label("name", clean(&title, 200)),
        label("dir", if cwd.is_empty() { path.to_string() } else { cwd.clone() }),
    ];
    if !files.is_empty() {
        out.push(label("files", cut(&files.join(" "), 220)));
    }
    out.push(label("file", path.replace(&home().to_string_lossy().to_string(), "~")));
    let said = blocks(path, 0);
    let matched = if at == 0 { String::new() } else { read(path, 0, 0, at) };
    // in name mode the match IS the first message, and printing it twice wastes the pane
    if !matched.is_empty() && said.first() != Some(&matched) {
        out.push(format!("--- match, line {at} ---"));
        out.push(cut(&matched, 1200));
    }
    out.push(String::new());
    for block in ends(&said, 2, 6) {
        // the pane renders ansi, so the role lines can carry the structure
        let shown = cut(&block, 700);
        out.push(match shown.split_once('\n') {
            Some((role, body)) => format!("\x1b[1m{role}\x1b[0m\n{body}"),
            None => shown,
        });
    }
    out.join("\n")
}

/// Files the session named in a tool call, read from the raw records: each agent has its own key.
fn files_named(path: &str) -> Vec<String> {
    let raw = match Path::new(path).file_stem().unwrap_or_default().to_string_lossy() {
        // opencode keeps the tool calls in its part shards, not in the session file
        stem if stem.starts_with("ses_") => opencode_messages(path)
            .into_iter()
            .flat_map(|(_, parts)| parts)
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::from_utf8_lossy(&std::fs::read(path).unwrap_or_default()).into_owned(),
    };
    let mut seen: Vec<String> = Vec::new();
    for found in FILE_ARG.captures_iter(&raw) {
        let name = &found[1];
        let short = name.rsplit('/').next().unwrap_or(name).to_string();
        if !short.is_empty() && !seen.contains(&short) {
            seen.push(short);
        }
    }
    seen.truncate(12);
    seen
}

/// One block per message, in order.
fn blocks(path: &str, at: u64) -> Vec<String> {
    let stem = Path::new(path).file_stem().unwrap_or_default().to_string_lossy().to_string();
    if stem.starts_with("ses_") {
        // opencode: a session is a directory of shards
        return opencode_transcript(path);
    }
    let mut blocks: Vec<String> = Vec::new();
    let raw = std::fs::read(path).unwrap_or_default();
    for (index, line) in String::from_utf8_lossy(&raw).split('\n').enumerate() {
        let line_no = index as u64 + 1;
        if at != 0 && line_no != at {
            continue;
        }
        let Some(block) = block(line, at != 0) else { continue };
        // a resent prompt is filed twice, once cut short, so keep whichever says more
        match blocks.last() {
            Some(last) if block.starts_with(last.as_str()) => drop(blocks.pop()),
            Some(last) if last.starts_with(&block) => continue,
            _ => {}
        }
        blocks.push(block);
    }
    if blocks.is_empty() {
        // not jsonl: gemini's logs.json is one array for the whole project
        if let Some(entry) = parse(&String::from_utf8_lossy(&raw)) {
            let said: Vec<String> = texts(&entry)
                .into_iter()
                .filter(|t| !t.trim().is_empty())
                .collect();
            blocks.push(said.join("\n"));
        }
    }
    blocks
}

/// One record as `--- role ---\nwhat was said`; `verbatim` keeps preambles and tool calls too.
fn block(line: &str, verbatim: bool) -> Option<String> {
    let entry = parse(line)?;
    if !verbatim
        && (entry.get("toolUseResult").is_some() || entry.get("isMeta") == Some(&Value::Bool(true)))
    {
        // claude files a tool result as a user turn, and echoes the prompt as an isMeta one
        return None;
    }
    let role = role_of(&entry);
    let said: Vec<String> = texts(&entry)
        .into_iter()
        .filter(|t| !t.trim().is_empty())
        .filter(|t| verbatim || !is_junk(t))
        .collect();
    let mut body = said.join("\n");
    if verbatim && body.is_empty() {
        // the match landed in a tool call; show the record itself
        body = cut(&serde_json::to_string_pretty(&entry).unwrap_or_default(), 4000);
    }
    if body.is_empty() || !(verbatim || role == "user" || role == "assistant") {
        return None;
    }
    Some(format!("--- {role} ---\n{body}"))
}

/// The first `head` and last `tail` blocks, with a marker for the dropped middle. 0,0 is all.
fn ends(blocks: &[String], head: usize, tail: usize) -> Vec<String> {
    if head + tail == 0 || blocks.len() <= head + tail {
        return blocks.to_vec();
    }
    let mut kept: Vec<String> = blocks[..head].to_vec();
    kept.push(format!("--- {} messages ---", blocks.len() - head - tail));
    kept.extend_from_slice(&blocks[blocks.len() - tail..]);
    kept
}
