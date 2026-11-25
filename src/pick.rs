//! Skim picker for sessions and agent filters. -- PI/OpenAI

use crate::sessions::{SOURCES, resume_for_path, session_of, store_summary};
use crate::transcript::{self, ReadOptions};
use skim::prelude::*;
use skim::reader::CommandCollector;
use std::io::{BufRead, Cursor};
use std::process::{Command, Stdio};

/// Rows the picker holds. Every session fits: fuzzy matching a loaded row is what makes the
/// name mode instant, and a row it never loaded is a row you cannot find.
pub const ROWS: usize = 20000;

/// The row, padded to fixed widths so the header lines up with it.
pub const COLUMNS: [(&str, usize); 5] = [
    ("when", 16),
    ("agent", 8),
    ("project", 16),
    ("name", 36),
    ("opening", 44),
];

pub fn header_row() -> String {
    COLUMNS
        .iter()
        .map(|(label, width)| format!("{label:width$}"))
        .collect::<Vec<_>>()
        .join(" ")
}

const SEARCH: &str = "asf-transcript-query:";

fn shell_arg(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

struct Collector {
    reader: SkimItemReader,
    search: String,
}

impl CommandCollector for Collector {
    fn invoke(&mut self, cmd: &str, stop: Arc<AtomicUsize>) -> (SkimItemReceiver, Sender<i32>) {
        if let Some(query) = cmd.strip_prefix(SEARCH) {
            return self
                .reader
                .invoke(&format!("{} {}", self.search, shell_arg(query)), stop);
        }
        self.reader.invoke(cmd, stop)
    }
}

const ALL: &str = "All agents";

fn choose_agent(current: Option<&str>, sub: bool) -> Option<&str> {
    let choices = std::iter::once(format!("{ALL:<12}\t"))
        .chain(
            SOURCES
                .iter()
                .map(|&(agent, _)| format!("{agent:<12}\t{}", store_summary(agent, sub))),
        )
        .collect::<Vec<_>>()
        .join("\n");
    let options = SkimOptions {
        layout: "reverse".into(),
        delimiter: "\t".into(),
        exact: true,
        prompt: "agent> ".into(),
        header: Some(format!(
            "Filter agent (current: {})\nType or use Up/Down | Enter apply | Esc cancel",
            current.unwrap_or(ALL)
        )),
        ..Default::default()
    };
    let items = SkimItemReader::new(
        SkimItemReaderOption::default()
            .delimiter("\t")
            .nth(["1"].into_iter()),
    )
    .of_bufread(Cursor::new(choices));
    let Some(out) = Skim::run_with(&options, Some(items)).filter(|out| !out.is_abort) else {
        return current;
    };
    let Some(chosen) = out.selected_items.first() else {
        return current;
    };
    let label = chosen.output();
    let name = label.split('\t').next().unwrap().trim_end();
    SOURCES
        .iter()
        .map(|&(agent, _)| agent)
        .find(|&agent| agent == name)
}

pub fn pick(
    source: Box<dyn BufRead + Send>,
    filters: &str,
    query: &str,
    mut agent: Option<&str>,
    mut content: bool,
    sub: bool,
) {
    let bat = Command::new("bat")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok();
    let me = std::env::current_exe().expect("cannot find my own path");
    let me = shell_arg(&me.to_string_lossy());
    let color = !std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
    let (heading, reset) = if color {
        ("\x1b[1;36m", "\x1b[0m")
    } else {
        ("", "")
    };
    let help = format!(
        "{heading}Picker shortcuts{reset}\n
{heading}Output{reset}
  Enter           Print resume command and close without running it
  Alt-P           Print transcript path and close
  Alt-M           Export Markdown to stdout and close

{heading}Search{reset}
  Ctrl-Q          Switch name / transcript search

{heading}Agent filters{reset}
  Ctrl-F          Choose an agent or All agents

{heading}Navigation{reset}
  Up/Down         Select a session; PgUp/PgDn move by page
  Shift-Up/Down   Scroll the transcript preview
  Esc             Close without printing

Press q to return to the picker."
    );
    let help_command = format!(
        "exec </dev/tty >/dev/tty; old=$(stty -g) || exit; trap 'stty \"$old\"' 0; trap 'exit 1' HUP INT TERM; stty -icanon -echo min 1 time 0 || exit; printf '\\n%s\\n' {}; while key=$(dd bs=1 count=1 2>/dev/null); do case \"$key\" in q|'') break;; esac; done",
        shell_arg(&help)
    );

    let reader = SkimItemReader::new(
        SkimItemReaderOption::default()
            .ansi(true)
            .delimiter("\t")
            .with_nth(["1"].into_iter()),
    );
    let mut items = Some(reader.of_bufread(source));
    let collector = Rc::new(RefCell::new(Collector {
        reader,
        search: String::new(),
    }));

    let mut options = SkimOptions {
        delimiter: "\t".to_string(),
        no_multi: true,
        ansi: true,
        expect: ["alt-p", "alt-m", "ctrl-f", "ctrl-q"]
            .map(str::to_string)
            .to_vec(),
        // default layout draws the list bottom-up, where page-down is a no-op at the newest row
        layout: "reverse".to_string(),
        // rows arrive newest first, and that beats a fuzzy score: you are usually after a
        // session from this week, not the one whose name matches most tightly
        tiebreak: vec![RankCriteria::Index],
        // substring, like the command line. Scattered fuzzy hits turned "intercom" into 42 rows
        exact: true,
        prompt: "name> ".to_string(),
        cmd_prompt: "transcript> ".to_string(),
        preview: Some(if bat {
            format!(
                "{me} --preview {{2}} --line {{3}} 2>&1 | awk 'BEGIN {{ body = 0; bat = \"bat --color=always --paging=never --plain --language=markdown\" }} !body {{ print; if ($0 == \"\") body = 1; next }} {{ print | bat }} END {{ if (body) close(bat) }}'"
            )
        } else {
            format!("{me} --preview {{2}} --line {{3}}")
        }),
        preview_window: "down:65%:wrap".to_string(),
        bind: vec![format!("?:execute[sh -c {}]", shell_arg(&help_command))],
        cmd_collector: collector.clone(),
        ..Default::default()
    };

    let mut query = query.to_owned();
    let out = loop {
        let agent_arg = agent.map_or_else(String::new, |name| format!(" -a {}", shell_arg(name)));
        let rows = format!("{me} --rows{filters}{agent_arg} -n {ROWS}");
        collector.borrow_mut().search = format!("{me} --rows{filters}{agent_arg} -n {ROWS} -c --");
        options.interactive = content;
        options.cmd = Some(if content {
            format!("{SEARCH}{{}}")
        } else {
            rows
        });
        options.query = (!content).then(|| query.clone());
        options.cmd_query = content.then(|| query.clone());
        options.header = Some(format!(
            "Output: Enter print resume command | Alt-P print path | Alt-M export Markdown\nSearch: Ctrl-Q switch mode | Ctrl-F filter agent ({})\nNavigation: Up/Down select | Shift-Up/Down scroll preview | Esc close | ? help\n{}",
            agent.unwrap_or(ALL),
            header_row()
        ));
        let Some(out) = Skim::run_with(&options, items.take()).filter(|out| !out.is_abort) else {
            return;
        };
        query = if content {
            out.cmd.clone()
        } else {
            out.query.clone()
        };
        match out.final_key {
            Key::Ctrl('f') => agent = choose_agent(agent, sub),
            Key::Ctrl('q') => content = !content,
            _ => break out,
        }
    };
    let Some(chosen) = out.selected_items.first() else {
        return;
    };
    let fields: Vec<String> = chosen.output().split('\t').map(str::to_string).collect();
    if fields.len() < 3 {
        return;
    }
    if out.final_key == Key::Alt('p') {
        println!("{}", fields[1]);
        return;
    }
    // Alt-M writes the selected transcript to stdout for redirection.
    if out.final_key == Key::Alt('m') {
        match transcript::read(&session_of(&fields[1]), ReadOptions::default()) {
            Ok(text) => println!("{text}"),
            Err(error) => {
                eprintln!("asf: {error}");
                std::process::exit(1);
            }
        }
        return;
    }
    // read the session fresh: after a transcript search these rows are ones the caller never scanned
    println!("{}", resume_for_path(&fields[1]));
}
