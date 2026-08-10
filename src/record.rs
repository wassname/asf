//! One transcript line, read. Every agent nests its text differently, so the walk is
//! generic: find the `text` keys, skip the tool calls, skip what the harness pasted in.

use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

pub static TAGS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<[a-z][a-z-]*>|</[a-z][a-z-]*>").unwrap());
pub static UUID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}").unwrap());
pub static SES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"ses_[A-Za-z0-9]+").unwrap());

/// preambles that are not what the session is about
pub const JUNK: [&str; 17] = [
    "<local-command-caveat>",
    "<system-reminder>",
    "Caveat: The messages below",
    "# AGENTS.md instructions",
    "<user_instructions>",
    "<environment_context>",
    "<permissions instructions>",
    "This session is being continued from",
    "<session_context>",
    "<task-notification>",
    "<command-name>",
    "<local-command-stdout>",
    "Base directory for this skill",
    "Stop hook feedback:",
    "<recommended_plugins>",
    "<bash-input>",
    "<INSTRUCTIONS>",
];

/// what the session did, not what it said
const SKIP_BLOCKS: [&str; 8] = [
    "tool_use",
    "tool_result",
    "thinking",
    "redacted_thinking",
    "image",
    "function_call",
    "function_call_output",
    "reasoning",
];

pub fn parse(raw: &str) -> Option<Value> {
    serde_json::from_str(raw).ok()
}

pub fn is_junk(text: &str) -> bool {
    let head = text.trim_start();
    JUNK.iter().any(|j| head.starts_with(j))
}

/// The first `n` characters, on a character boundary.
pub fn head(text: &str, n: usize) -> &str {
    match text.char_indices().nth(n) {
        Some((byte, _)) => &text[..byte],
        None => text,
    }
}

pub fn cut(text: &str, n: usize) -> String {
    head(text, n).to_string()
}

/// What was said in a transcript entry, whatever the agent's schema.
///
/// Claude, codex and pi all end up at a `text` key. Claude also writes a plain string
/// content for simple user turns, so take that too.
pub fn texts(entry: &Value) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![entry];
    while let Some(node) = stack.pop() {
        match node {
            Value::Object(map) => {
                if let Some(Value::String(kind)) = map.get("type")
                    && SKIP_BLOCKS.contains(&kind.as_str()) {
                        continue;
                    }
                for key in ["text", "content", "message"] {
                    if let Some(Value::String(said)) = map.get(key) {
                        found.push(said.clone());
                    }
                }
                stack.extend(map.values());
            }
            Value::Array(items) => stack.extend(items.iter()),
            _ => {}
        }
    }
    found
}

/// First value for `key` anywhere in the record. The agents nest it differently: claude
/// has cwd at the top, codex under payload, copilot under data.context.
pub fn find_value(entry: &Value, key: &str) -> String {
    let mut stack = vec![entry];
    while let Some(node) = stack.pop() {
        match node {
            Value::Object(map) => {
                if let Some(Value::String(found)) = map.get(key) {
                    return found.clone();
                }
                stack.extend(map.values());
            }
            Value::Array(items) => stack.extend(items.iter()),
            _ => {}
        }
    }
    String::new()
}

/// user / assistant / whatever the harness called this record.
pub fn role_of(entry: &Value) -> String {
    let role = find_value(entry, "role");
    if !role.is_empty() {
        return role;
    }
    // copilot writes assistant.message, gemini writes user
    let kind = entry.get("type").and_then(Value::as_str).unwrap_or("?");
    kind.split('.').next().unwrap_or("?").to_string()
}

pub fn clean(text: &str, width: usize) -> String {
    let stripped = TAGS.replace_all(text, " ");
    cut(&stripped.split_whitespace().collect::<Vec<_>>().join(" "), width)
}

/// The part of the text around the match, so the row shows why it matched.
pub fn window(text: &str, query: &str, width: usize) -> String {
    let stripped = TAGS.replace_all(text, " ").to_string();
    let literal = Regex::new(&format!("(?i){}", regex::escape(query)))
        .ok()
        .and_then(|re| re.find(&stripped));
    // the query as written, in case the caller passed a pattern. A pattern that does not
    // compile is not an error here: the window is decoration, the hit is already known.
    let found = literal.or_else(|| {
        Regex::new(&format!("(?is){query}"))
            .ok()
            .and_then(|re| re.find(&stripped))
    });
    let start = found.map_or(0, |m| {
        stripped[..m.start()]
            .chars()
            .count()
            .saturating_sub(width / 3)
    });
    let around: String = stripped.chars().skip(start).take(width * 2).collect();
    cut(&around.split_whitespace().collect::<Vec<_>>().join(" "), width)
}

static ATTACHMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""(attachment|skill_listing)""#).unwrap());
// a record that is the harness talking to itself, not a turn in the conversation:
// codex pastes AGENTS.md into every transcript, and a session header holds the cwd
static PREAMBLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r#"^\{"type": ?"session"#,
        r#"|"type": ?"(world_state|turn_context)"|"agents_md""#,
        r#"|"(text|content)": ?"(<recommended_plugins>|<user_instructions>|<environment_context>"#,
        r#"|<INSTRUCTIONS>|# AGENTS\.md instructions|<system-reminder>|<local-command-caveat>"#,
        r#"|Caveat: The messages below|Stop hook feedback:)"#,
    ))
    .unwrap()
});
const INJECTED: &str = "system-reminder|skills_instructions|available-skills|env|task-notification";
static OPENS: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("<({INJECTED})>")).unwrap());
static CLOSES: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("</({INJECTED})>")).unwrap());

/// True when the match sits in text the harness pastes into every session: the skill list,
/// AGENTS.md, a session header. Those match most transcripts and mean nothing.
///
/// Without this, `asf -c "Virgin Defensive"` returned 411 sessions for a phrase nobody ever
/// said, because it is a row in the AGENTS.md table codex prepends to every run.
pub fn injected(raw: &str, query: &str) -> bool {
    if ATTACHMENT.is_match(head(raw, 400)) || PREAMBLE.is_match(head(raw, 2000)) {
        return true;
    }
    let Some(found) = Regex::new(&format!("(?i){}", regex::escape(query)))
        .ok()
        .and_then(|re| re.find(raw))
    else {
        return false;
    };
    let before = &raw[..found.start()];
    let opens = OPENS.find_iter(before).map(|m| m.start() as i64).max();
    let closes = CLOSES
        .find_iter(before)
        .map(|m| m.start() as i64)
        .max()
        .unwrap_or(-1);
    opens.is_some_and(|open| open > closes)
}
