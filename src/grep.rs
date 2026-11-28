//! Message search with source record numbers and surrounding turns. -- PI/OpenAI

use crate::{
    Role, matches_directory, normalize_path,
    record::{Show, clean},
    sessions, transcript,
};
use regex::{Match, Regex, RegexBuilder};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct Args {
    /// Literal text to find (case insensitive).
    #[arg(required = true)]
    query: Vec<String>,
    /// Treat the query as a regular expression.
    #[arg(long = "re")]
    regex: bool,
    /// Search sessions recorded in this exact directory; repeat for multiple directories.
    #[arg(long, value_parser = normalize_path)]
    cwd: Vec<PathBuf>,
    /// Only match messages with this role; context may include other roles.
    #[arg(long, value_enum)]
    role: Option<Role>,
    /// Only search this agent's sessions.
    #[arg(short, long)]
    agent: Option<String>,
    /// Include subagent sessions.
    #[arg(long)]
    sub: bool,
    /// Show N messages before each hit.
    #[arg(short = 'B', long = "before-context", default_value_t = 0)]
    before: usize,
    /// Show N messages after each hit.
    #[arg(short = 'A', long = "after-context", default_value_t = 0)]
    after: usize,
    /// Highlight matching text on a terminal by default.
    #[arg(long, value_enum, default_value = "auto")]
    color: Color,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Color {
    Auto,
    Always,
    Never,
}

fn single_line(text: &str) -> String {
    text.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

fn append_body(out: &mut String, text: &str, multiline: bool) {
    for ch in text.chars() {
        match ch {
            '\n' if multiline => out.push_str("\n  "),
            '\t' if multiline => out.push(ch),
            _ if ch.is_control() => out.push(' '),
            _ => out.push(ch),
        }
    }
}

fn render_body(
    text: &str,
    hit: Option<Match<'_>>,
    highlight: Option<&Regex>,
    full: bool,
) -> String {
    let (start, end) = if full {
        (0, text.len())
    } else {
        let start = hit
            .and_then(|m| text[..m.start()].char_indices().rev().nth(39))
            .map_or(0, |(index, _)| index);
        let end = text[start..]
            .char_indices()
            .nth(160)
            .map_or(text.len(), |(index, _)| start + index);
        (start, end)
    };
    let mut out = if start == 0 {
        String::new()
    } else {
        "…".into()
    };
    let mut cursor = start;
    if let Some(pattern) = highlight.filter(|_| hit.is_some()) {
        for m in pattern.find_iter(text).take_while(|m| m.start() < end) {
            if m.end() <= start || m.is_empty() {
                continue;
            }
            append_body(&mut out, &text[cursor..m.start().max(start)], full);
            out.push_str("\x1b[1;31m");
            cursor = m.end().min(end);
            append_body(&mut out, &text[m.start().max(start)..cursor], full);
            out.push_str("\x1b[0m");
        }
    }
    append_body(&mut out, &text[cursor..end], full);
    if end < text.len() {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_context_keeps_long_multiline_messages() {
        let prefix = "é".repeat(180);
        let text = format!("{prefix}\n\nTry J-lens\n    final line");
        let pattern = Regex::new("J-lens").unwrap();
        assert_eq!(
            render_body(&text, pattern.find(&text), Some(&pattern), true),
            format!("{prefix}\n  \n  Try \x1b[1;31mJ-lens\x1b[0m\n      final line")
        );
        assert_eq!(
            render_body(&text, None, None, false),
            format!("{}…", "é".repeat(160))
        );
    }
}

pub fn run(args: Args) -> i32 {
    if let Some(agent) = &args.agent
        && !sessions::SOURCES.iter().any(|(source, _)| source == agent)
    {
        eprintln!("asf: no such agent {agent:?}");
        return 2;
    }
    let query = args.query.join(" ");
    if query.is_empty() {
        eprintln!("asf grep: provide a nonempty pattern");
        return 2;
    }
    let wanted = if args.regex {
        query
    } else {
        regex::escape(&query)
    };
    let pattern = match RegexBuilder::new(&wanted).case_insensitive(true).build() {
        Ok(pattern) => pattern,
        Err(error) => {
            eprintln!("asf grep: {error}");
            return 2;
        }
    };
    let checked = RefCell::new(HashMap::new());
    let in_cwd = |recorded: &str| {
        *checked
            .borrow_mut()
            .entry(recorded.to_owned())
            .or_insert_with(|| args.cwd.iter().any(|cwd| matches_directory(recorded, cwd)))
    };
    let terminal = io::stdout().is_terminal();
    let color = match args.color {
        Color::Always => true,
        Color::Never => false,
        Color::Auto => terminal && std::env::var_os("NO_COLOR").is_none(),
    };
    let show = Show {
        tools: matches!(args.role, Some(Role::Tool)),
        think: false,
    };
    let mut stdout = io::stdout().lock();
    let mut matched = false;
    let mut failed = false;
    let context = args.before > 0 || args.after > 0;
    let result = sessions::visit_transcripts(
        args.agent.as_deref(),
        args.sub,
        (!args.cwd.is_empty()).then_some(&in_cwd),
        |row| {
            let messages = match transcript::messages_in(row, 0, show) {
                Ok(messages) => messages,
                Err(error) => {
                    eprintln!("asf grep: {error}");
                    failed = true;
                    return Ok(());
                }
            };
            let hits: Vec<_> = messages
                .iter()
                .map(|message| {
                    args.role
                        .is_none_or(|role| role.as_str() == message.role)
                        .then(|| pattern.find(&message.text))
                        .flatten()
                })
                .collect();
            let mut printed = 0;
            for (index, _) in hits.iter().enumerate().filter(|(_, hit)| hit.is_some()) {
                let first = index.saturating_sub(args.before);
                let end = index
                    .saturating_add(args.after)
                    .saturating_add(1)
                    .min(messages.len());
                if terminal && printed == 0 {
                    if matched {
                        writeln!(stdout)?;
                    }
                    let title = if row.title.is_empty() {
                        messages
                            .iter()
                            .find(|m| m.role == "user")
                            .map_or("", |m| &m.text)
                    } else {
                        &row.title
                    };
                    let title = single_line(&clean(title, 72));
                    let title = if title.is_empty() {
                        "(untitled)"
                    } else {
                        &title
                    };
                    writeln!(stdout, "## Session: {title}\n\n`{}`\n", row.path)?;
                } else if context && matched && (printed == 0 || first > printed) {
                    writeln!(stdout, "{}", if terminal { "" } else { "--" })?;
                }
                matched = true;
                for (message, hit) in messages[first.max(printed)..end]
                    .iter()
                    .zip(&hits[first.max(printed)..end])
                {
                    let body = render_body(
                        &message.text,
                        *hit,
                        color.then_some(&pattern),
                        terminal && context,
                    );
                    let number = message.number();
                    let role = &message.role;
                    if terminal {
                        let label = if hit.is_some() {
                            format!("**Match #{number}**")
                        } else {
                            format!("Context #{number}")
                        };
                        let time = message
                            .timestamp
                            .map_or_else(String::new, |t| format!(", {t}"));
                        writeln!(stdout, "- {label} ({role}{time}): {body}")?;
                    } else {
                        let marker = if hit.is_some() { '|' } else { '-' };
                        let date = message
                            .timestamp
                            .map_or_else(|| "-".into(), |t| t.to_string());
                        writeln!(
                            stdout,
                            "{}:{number} {marker} {date} | {role} | {body}",
                            row.path
                        )?;
                    }
                }
                printed = end;
            }
            Ok(())
        },
    );
    if let Err(error) = result {
        eprintln!("asf grep: {error}");
        failed = true;
    }
    if failed {
        2
    } else if matched {
        0
    } else {
        1
    }
}
