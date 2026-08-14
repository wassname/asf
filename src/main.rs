//! asf: find a past coding-agent session by its name, or by anything said inside it.
//!
//!     asf                      the newest sessions
//!     asf steer                sessions whose NAME matches (default)
//!     asf -c "staging dir"     sessions whose TRANSCRIPT matches, assistant text included
//!     asf -i steer             pick one in skim; enter prints the resume command
//!     asf --paths -c steer     just the transcript paths, for piping
//!
//! No index. The scan reads all 3.4 GB of transcripts in about a second, so there is nothing
//! to build and nothing to go stale. Stopping each file at its first hit is what keeps a
//! common word from returning 200k rows.

mod pick;
mod record;
mod scan;
mod sessions;

use clap::Parser;
use sessions::{Row, SOURCES};
use std::path::Path;

#[derive(Parser)]
#[command(
    name = "asf",
    about = "Find a past coding-agent session by name, or by anything said inside it",
    after_help = "\
Name search, the default, matches the session's own name, its project, and the first thing
you said. Content search, -c, reads every message, what the assistant said and what tools
printed included. Both take a literal phrase and ignore case; --re opts into a pattern.

A name is whichever the agent kept: the one you typed (claude /rename, a codex thread name,
a pi --session-id), then the one its UI shows, then your opening message. Where a session was
renamed part way through, this is the name it ended with.

Runs an agent started for itself are hidden, because you cannot resume them. --sub shows them.
For pi that means every session with a name, since pi gives its own a uuid and only a tool
passes --session-id.
The picker prints its own keys. README.md and RESEARCH_JOURNAL.md have the rest."
)]
struct Args {
    /// words to look for. Empty lists the newest sessions.
    query: Vec<String>,
    /// search the whole transcript, not the name
    #[arg(short, long)]
    content: bool,
    /// treat the query as a regular expression, not a phrase
    #[arg(long = "re")]
    regex: bool,
    /// choose one in skim, print its resume command
    #[arg(short = 'i', long = "pick")]
    pick: bool,
    /// only this agent
    #[arg(short, long)]
    agent: Option<String>,
    /// include claude subagent logs, which cannot be resumed
    #[arg(long)]
    sub: bool,
    /// rows to print
    #[arg(short = 'n', long, default_value_t = 20)]
    limit: usize,
    /// print transcript paths only
    #[arg(long)]
    paths: bool,
    /// the tab separated rows the picker gets, for checking
    #[arg(long)]
    rows: bool,
    /// print a transcript as text
    #[arg(long, value_name = "PATH")]
    read: Option<String>,
    /// one screen about a transcript: where it ran, files it named, its first and last words
    #[arg(long, value_name = "PATH")]
    preview: Option<String>,
    /// print the command that reopens a transcript
    #[arg(long, value_name = "PATH")]
    resume: Option<String>,
    /// with --read, only the first N messages
    #[arg(long, default_value_t = 0)]
    head: usize,
    /// with --read, only the last N messages
    #[arg(long, default_value_t = 0)]
    tail: usize,
    /// with --read or --preview, the record on that line
    #[arg(long, default_value_t = 0)]
    line: u64,
}

fn project(row: &Row) -> String {
    if !row.cwd.is_empty() {
        return Path::new(&row.cwd)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
    }
    let parent = Path::new(&row.path)
        .parent()
        .and_then(Path::file_name)
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let chars: Vec<char> = parent.chars().collect();
    chars[chars.len().saturating_sub(24)..].iter().collect()
}

fn or_dash(text: &str, width: usize) -> String {
    let short = record::cut(text, width);
    if short.is_empty() { "-".to_string() } else { short }
}

/// The column beside the name: why the row matched, or what you opened the session with.
fn said(row: &Row) -> &str {
    if !row.matched.is_empty() {
        return &row.matched;
    }
    // a name replaced the opening message in the title, so there is room to show both
    if row.opening != row.title { &row.opening } else { "" }
}

fn table(rows: &[Row], content: bool) -> String {
    let home = sessions::home().to_string_lossy().into_owned();
    let matched = rows.iter().any(|r| !said(r).is_empty());
    let mut head: Vec<String> = ["when", "agent", "project", "name"]
        .iter()
        .map(|h| h.to_string())
        .collect();
    if matched {
        head.push(if content { "match" } else { "opening" }.to_string());
    }
    head.push("file".to_string());

    let body: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            let mut cells = vec![
                sessions::day(r.mtime, "%Y-%m-%d %H:%M"),
                r.agent.clone(),
                record::cut(&project(r), 20),
                or_dash(&r.title, 60),
            ];
            if matched {
                cells.push(or_dash(said(r), 60));
            }
            cells.push(r.path.replace(&home, "~"));
            cells
        })
        .collect();

    let widths: Vec<usize> = (0..head.len())
        .map(|i| {
            std::iter::once(&head)
                .chain(body.iter())
                .map(|row| row[i].chars().count())
                .max()
                .unwrap_or(0)
        })
        .collect();
    let pad = |cell: &str, width: usize| {
        format!("{cell}{}", " ".repeat(width.saturating_sub(cell.chars().count())))
    };
    let row_line = |cells: &Vec<String>| {
        let padded: Vec<String> = cells.iter().zip(&widths).map(|(c, w)| pad(c, *w)).collect();
        format!("| {} |", padded.join(" | "))
    };

    let mut lines = vec![
        row_line(&head),
        format!(
            "|{}|",
            widths.iter().map(|w| "-".repeat(w + 2)).collect::<Vec<_>>().join("|")
        ),
    ];
    lines.extend(body.iter().map(row_line));
    lines.join("\n")
}

/// One padded block for the eye, then the path and line for the preview and for resuming.
fn rows_tsv(rows: &[Row]) -> String {
    rows.iter()
        .map(|r| {
            let cells = [
                sessions::day(r.mtime, "%Y-%m-%d %H:%M"),
                r.agent.clone(),
                project(r),
                or_dash(&r.title, 60),
                or_dash(said(r), 70),
            ];
            let shown: Vec<String> = cells
                .iter()
                .zip(pick::COLUMNS)
                .map(|(cell, (_, width))| format!("{:width$}", record::cut(cell, width)))
                .collect();
            format!(
                "{}\t{}\t{}",
                shown.join(" "),
                if r.hit.is_empty() { &r.path } else { &r.hit },
                r.line
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn main() {
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) }; // let `| head` close the pipe quietly
    let args = Args::parse();

    if let Some(path) = &args.read {
        println!("{}", sessions::read(path, args.head, args.tail, args.line));
        return;
    }
    if let Some(path) = &args.preview {
        println!("{}", sessions::preview(path, args.line));
        return;
    }
    if let Some(path) = &args.resume {
        println!("{}", sessions::resume_for_path(path));
        return;
    }

    // a store that moved or got renamed would otherwise just go quiet
    for (agent, _) in SOURCES {
        let store = sessions::store(agent);
        if !store.exists() {
            eprintln!("asf: no {agent} store at {}", store.display());
        }
    }

    let query = args.query.join(" ");
    let mut rows = if args.content && !query.is_empty() {
        sessions::search_content(&query, args.regex)
    } else {
        let mut rows = sessions::load_sessions();
        if !query.is_empty() {
            let wanted = if args.regex { query.clone() } else { regex::escape(&query) };
            let pattern = match regex::Regex::new(&format!("(?i){wanted}")) {
                Ok(pattern) => pattern,
                Err(err) => {
                    eprintln!("asf: bad pattern {query:?}: {err}");
                    std::process::exit(1);
                }
            };
            // the opening message too: a name is what the agent called the session, and you
            // are more likely to remember what you asked for
            rows.retain(|r| {
                pattern.is_match(&r.title)
                    || pattern.is_match(&r.opening)
                    || pattern.is_match(&r.path)
                    || pattern.is_match(&r.cwd)
            });
        }
        rows
    };

    if let Some(agent) = &args.agent {
        if !SOURCES.iter().any(|(a, _)| a == agent) {
            eprintln!("asf: no such agent {agent:?}");
            std::process::exit(2);
        }
        rows.retain(|r| &r.agent == agent);
    }
    if !args.sub {
        if args.content {
            sessions::mark_subagents(&mut rows); // name mode marked them as it read the headers
        }
        rows.retain(|r| !r.sub);
    }
    rows.sort_by(|a, b| b.mtime.total_cmp(&a.mtime));

    let total = rows.len();
    if args.pick {
        rows.truncate(pick::ROWS);
        sessions::hydrate(&mut rows, &query);
        // the picker reruns me for its transcript search, so it needs the same filters back
        let mut filters = String::new();
        if let Some(agent) = &args.agent {
            filters.push_str(&format!(" -a {agent}"));
        }
        if args.sub {
            filters.push_str(" --sub");
        }
        pick::pick(rows_tsv(&rows), &filters, &query);
    } else if args.rows {
        rows.truncate(args.limit);
        sessions::hydrate(&mut rows, &query);
        println!("{}", rows_tsv(&rows));
    } else if args.paths {
        rows.truncate(args.limit);
        let paths: Vec<String> = rows.iter().map(|r| r.path.clone()).collect();
        println!("{}", paths.join("\n"));
    } else if total > 0 {
        rows.truncate(args.limit);
        sessions::hydrate(&mut rows, &query);
        println!("{}", table(&rows, args.content));
        println!("\n{total} sessions matched, showing {}", total.min(args.limit));
    } else {
        println!("nothing matched");
    }
}
