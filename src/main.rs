//! asf: find a past coding-agent session by its name, or by anything said inside it.
//!
//!     asf                      the newest sessions
//!     asf steer                sessions whose NAME matches (default)
//!     asf -c "staging dir"     sessions whose TRANSCRIPT matches, assistant text included
//!     asf -i steer             pick one in skim; enter prints the resume command
//!     asf --read 019ffeb2      that session as markdown, by the id its resume command shows
//!     asf --paths -c steer     just the transcript paths, for piping
//!

mod hermes;
mod pick;
mod record;
mod scan;
mod sessions;

use clap::{Parser, ValueEnum};
use sessions::{Row, SOURCES};
use std::io::{BufReader, BufWriter, Cursor, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, ValueEnum)]
enum Role {
    User,
    Assistant,
    Tool,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

#[derive(Parser)]
#[command(
    name = "asf",
    about = "Find a past coding-agent session by name, or by anything said inside it",
    after_help = "\
Examples:
  asf lucid                  list sessions whose name, project or first message says lucid
  asf -c \"staging dir\"       search everything said in each session, not only the name
  asf lucid -r --tail 40     read the newest match as markdown, last 40 messages
  asf -r 019ffeb2            read by session id, a prefix is enough
  asf -r <file.jsonl>        read by transcript path
  asf -p lucid               one-screen preview of the newest match
  asf -u lucid               print the command that reopens it, and so its id

-n is --limit (rows to print), not a name: the name is the plain query word.

Name search, the default, matches the session's own name, its project, and the first thing
you said. Content search, -c, reads every message, what the assistant said and what tools
printed included. Both take a literal phrase and ignore case; --re opts into a pattern.

A name is whichever the agent kept: the one you typed (claude /rename, a codex thread name,
a pi --session-id), then the one its UI shows, then your opening message. Where a session was
renamed part way through, this is the name it ended with.

Runs an agent started for itself are hidden, because you cannot resume them. --sub shows them.
For pi that means every session with a name, since pi gives its own a uuid and only a tool
passes --session-id.

Read your current Pi transcript when you need exact earlier wording, decisions, or context:
`asf -r \"$PI_INTERCOM_SESSION_ID\" --tail 40`.

-r/--read exports the session as markdown, `## role` a message, the conversation only. --tools
and --think put the tool calls and the reasoning back, a line each; --head and --tail cut it.
--role user, assistant, or tool filters records before --head and --tail. --role tool enables
--tools: it selects tool records, while --tools also retains tool details in user and assistant
records. SESSION is the id in the resume command asf just printed you, which is also the id your
agent shows. `asf -u lucid` says `codex resume 019ffeb2-9c72-7ad0`, so `asf -r 019ffeb2` or
`asf -p 019ffeb2` reads that same session back. A transcript path works too, and so does nothing
at all, which takes the newest session the query matched: `asf lucid -r`.

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
    #[arg(long, hide = true)]
    stream_rows: bool,
    /// print a session as markdown. Nothing given: the newest session the query matched
    #[arg(short = 'r', long, value_name = "SESSION", num_args = 0..=1, default_missing_value = "")]
    read: Option<String>,
    /// one screen about a session: where it ran, its model, files it named, first and last words
    #[arg(short = 'p', long, value_name = "SESSION", num_args = 0..=1, default_missing_value = "")]
    preview: Option<String>,
    /// print the command that reopens a session
    #[arg(short = 'u', long, value_name = "SESSION", num_args = 0..=1, default_missing_value = "")]
    resume: Option<String>,
    /// with --read, keep only user, assistant, or tool records; --role tool also enables --tools
    #[arg(long, value_enum, requires = "read")]
    role: Option<Role>,
    /// with --read, only the first N messages
    #[arg(long, default_value_t = 0)]
    head: usize,
    /// with --read, only the last N messages
    #[arg(long, default_value_t = 0)]
    tail: usize,
    /// with --read, keep the tool calls and their results, a line each
    #[arg(long)]
    tools: bool,
    /// with --read, keep the reasoning blocks, quoted
    #[arg(long)]
    think: bool,
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
    if short.is_empty() {
        "-".to_string()
    } else {
        short
    }
}

/// The column beside the name: why the row matched, or what you opened the session with.
fn said(row: &Row) -> &str {
    if !row.matched.is_empty() {
        return &row.matched;
    }
    // a name replaced the opening message in the title, so there is room to show both
    if row.opening != row.title {
        &row.opening
    } else {
        ""
    }
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
        format!(
            "{cell}{}",
            " ".repeat(width.saturating_sub(cell.chars().count()))
        )
    };
    let row_line = |cells: &Vec<String>| {
        let padded: Vec<String> = cells.iter().zip(&widths).map(|(c, w)| pad(c, *w)).collect();
        format!("| {} |", padded.join(" | "))
    };

    let mut lines = vec![
        row_line(&head),
        format!(
            "|{}|",
            widths
                .iter()
                .map(|w| "-".repeat(w + 2))
                .collect::<Vec<_>>()
                .join("|")
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

fn stream_rows(
    query: &str,
    regex: bool,
    agent: Option<&str>,
    sub: bool,
    stop: &AtomicBool,
    output: impl Write,
) {
    let mut writer = BufWriter::new(output);
    let mut count = 0;
    sessions::stream_names(query, regex, agent, sub, stop, |row| {
        count += 1;
        count <= pick::ROWS
            && writeln!(writer, "{}", rows_tsv(&[row])).is_ok()
            && writer.flush().is_ok()
    });
}

/// Whichever of --read/--preview/--resume was asked for, against one transcript.
fn one(args: &Args, path: &str) -> ! {
    let out = if args.read.is_some() {
        let show = record::Show {
            tools: args.tools || matches!(args.role, Some(Role::Tool)),
            think: args.think,
        };
        sessions::read(
            path,
            args.head,
            args.tail,
            args.line,
            show,
            args.role.map(Role::as_str),
        )
    } else if args.preview.is_some() {
        sessions::preview(path, args.line)
    } else {
        sessions::resume_for_path(path)
    };
    if out.trim().is_empty() {
        eprintln!("asf: no session records in {path}");
        std::process::exit(1);
    }
    println!("{out}");
    std::process::exit(0);
}

fn main() {
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) }; // let `| head` close the pipe quietly
    let args = Args::parse();
    if let Some(agent) = &args.agent {
        if !SOURCES.iter().any(|(a, _)| a == agent) {
            eprintln!("asf: no such agent {agent:?}");
            std::process::exit(2);
        }
    }

    // a path given outright needs no scan; an empty one means "resolve it from the query"
    let one_of = [
        args.read.as_deref(),
        args.preview.as_deref(),
        args.resume.as_deref(),
    ];
    let wants_one = one_of.iter().any(|f| f.is_some());
    let given = one_of.into_iter().flatten().find(|p| !p.is_empty());
    // a hermes session is <db>#<id>, which is no file on disk
    if let Some(path) = given.filter(|p| Path::new(p).exists() || p.contains('#')) {
        one(&args, path);
    }
    // a missing path would fall through to the search below and scan every transcript for it
    if let Some(p) = given.filter(|p| p.contains('/') || p.ends_with(".jsonl") || p.ends_with(".json")) {
        eprintln!("asf: no such session file: {p}");
        std::process::exit(2);
    }

    // Resolve IDs before name search reads the transcript stores. -- PI/OpenAI
    let mut query = args.query.join(" ");
    if let Some(id) = given {
        if !query.is_empty() {
            eprintln!("asf: give a query or a session, not both: {query:?} and {id:?}");
            std::process::exit(2);
        }
        if !args.content && !args.regex {
            let paths = sessions::resolve_id(id, args.agent.as_deref(), args.sub);
            match paths.as_slice() {
                [path] => {
                    eprintln!("asf: session {path}");
                    one(&args, path);
                }
                [] if sessions::looks_like_id(id) => {
                    eprintln!("asf: no such session id: {id}");
                    std::process::exit(1);
                }
                [] => {}
                _ => {
                    eprintln!("asf: ambiguous session id {id:?}:\n{}", paths.join("\n"));
                    std::process::exit(2);
                }
            }
        }
        query = id.to_string();
    }
    if (args.pick || args.stream_rows) && !args.content {
        if args.regex && let Err(err) = regex::Regex::new(&format!("(?i){query}")) {
            eprintln!("asf: bad pattern {query:?}: {err}");
            std::process::exit(1);
        }
        if args.stream_rows {
            stream_rows(&query, args.regex, args.agent.as_deref(), args.sub, &AtomicBool::new(false), std::io::stdout().lock());
            return;
        }
        let (reader, writer) = UnixStream::pair().expect("cannot open picker stream");
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker_query = query.clone();
        let worker_agent = args.agent.clone();
        let sub = args.sub;
        let regex = args.regex;
        std::thread::spawn(move || {
            stream_rows(&worker_query, regex, worker_agent.as_deref(), sub, &worker_stop, writer);
        });
        let filters = match (&args.agent, args.sub) {
            (Some(agent), true) => format!(" -a {agent} --sub"),
            (Some(agent), false) => format!(" -a {agent}"),
            (None, true) => " --sub".to_string(),
            (None, false) => String::new(),
        };
        pick::pick(Box::new(BufReader::new(reader)), &filters, &query);
        stop.store(true, Ordering::Relaxed);
        return;
    }
    let mut rows = if args.content && !query.is_empty() {
        sessions::search_content(&query, args.regex)
    } else {
        let mut rows = sessions::load_sessions();
        if !query.is_empty() {
            let wanted = if args.regex {
                query.clone()
            } else {
                regex::escape(&query)
            };
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
    if wants_one {
        let Some(row) = rows.first() else {
            eprintln!("asf: nothing matched {query:?}");
            std::process::exit(1);
        };
        let action = if args.read.is_some() {
            "reading"
        } else if args.preview.is_some() {
            "previewing"
        } else {
            "resuming"
        };
        let shown = total.min(3);
        let selection = if total == 1 {
            "the only match".to_string()
        } else {
            format!("the newest of {total} matches (top {shown} shown)")
        };
        eprintln!(
            "asf: {action} {selection}:\n{}",
            table(&rows[..shown], args.content)
        );
        one(&args, &row.path.clone());
    }
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
        pick::pick(Box::new(Cursor::new(rows_tsv(&rows))), &filters, &query);
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
        println!(
            "\n{total} sessions matched, showing {}",
            total.min(args.limit)
        );
    } else {
        println!("nothing matched");
    }
}
