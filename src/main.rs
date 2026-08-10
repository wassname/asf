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
use jiff::Timestamp;
use jiff::tz::TimeZone;
use sessions::{Row, SOURCES};
use std::path::Path;

#[derive(Parser)]
#[command(
    name = "asf",
    about = "Find a past coding-agent session by name, or by anything said inside it"
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
    /// with --read, only the last N messages
    #[arg(long, default_value_t = 0)]
    tail: usize,
    /// with --read, only the record on that line
    #[arg(long, default_value_t = 0)]
    line: u64,
}

fn day(mtime: f64, format: &str) -> String {
    Timestamp::from_second(mtime as i64)
        .expect("mtime out of range")
        .to_zoned(TimeZone::system())
        .strftime(format)
        .to_string()
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

fn table(rows: &[Row], matched: bool) -> String {
    let home = sessions::home().to_string_lossy().into_owned();
    let mut head: Vec<String> = ["when", "agent", "project", "name"]
        .iter()
        .map(|h| h.to_string())
        .collect();
    if matched {
        head.push("match".to_string());
    }
    head.push("file".to_string());

    let body: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            let mut cells = vec![
                day(r.mtime, "%Y-%m-%d"),
                r.agent.clone(),
                record::cut(&project(r), 20),
                or_dash(&r.title, 60),
            ];
            if matched {
                cells.push(or_dash(&r.matched, 60));
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

fn rows_tsv(rows: &[Row]) -> String {
    rows.iter()
        .map(|r| {
            [
                day(r.mtime, "%m-%d"),
                r.agent.clone(),
                record::cut(&project(r), 18),
                or_dash(&r.title, 60),
                or_dash(&r.matched, 70),
                if r.hit.is_empty() { r.path.clone() } else { r.hit.clone() },
                r.line.to_string(),
            ]
            .join("\t")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn main() {
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) }; // let `| head` close the pipe quietly
    let args = Args::parse();

    if let Some(path) = &args.read {
        println!("{}", sessions::read(path, args.tail, args.line));
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
    let mut rows = if args.content {
        if query.is_empty() {
            eprintln!("-c needs something to search for");
            std::process::exit(1);
        }
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
            rows.retain(|r| {
                pattern.is_match(&r.title) || pattern.is_match(&r.path) || pattern.is_match(&r.cwd)
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
        rows.retain(|r| !r.path.contains("/subagents/"));
    }
    rows.sort_by(|a, b| b.mtime.total_cmp(&a.mtime));

    let total = rows.len();
    if args.pick {
        rows.truncate(400);
        sessions::hydrate(&mut rows, &query);
        pick::pick(rows_tsv(&rows));
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
