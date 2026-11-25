//! skim ranks loaded names; ctrl-q switches to a command that rescans every transcript.

use crate::sessions::{SOURCES, resume_for_path, session_of};
use crate::transcript::{self, ReadOptions};
use skim::prelude::*;
use skim::reader::CommandCollector;
use std::io::BufRead;
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

pub fn pick(source: Box<dyn BufRead + Send>, filters: &str, query: &str, agent: Option<&str>) {
    let bat = Command::new("bat")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok();
    let me = std::env::current_exe().expect("cannot find my own path");
    let me = shell_arg(&me.to_string_lossy());
    let search_agent = agent.map_or_else(String::new, |a| format!(" -a {a}"));
    let mut agents = Vec::new();
    let mut legend = Vec::new();
    for (n, (agent, _)) in SOURCES.iter().enumerate() {
        agents.push(format!(
            "f{}:reload({me} --rows{filters} -a {agent} -n {ROWS})",
            n + 1
        ));
        legend.push(format!("f{} {agent}", n + 1));
    }
    agents.push(format!(
        "f{}:reload({me} --rows{filters} -n {ROWS})",
        SOURCES.len() + 1
    ));
    legend.push(format!("f{} all", SOURCES.len() + 1));
    let help = format!(
        "Picker shortcuts\n\n\
        Output\n\
        Enter           Print resume command and close without running it\n\
        Alt-P           Print transcript path and close\n\
        Alt-M           Export Markdown to stdout and close\n\n\
        Search\n\
        Ctrl-Q          Switch name / transcript search\n\
        Agent filters: {}\n\n\
        Navigation\n\
        Up/Down         Select a session; PgUp/PgDn move by page\n\
        Ctrl-Up/Down    Scroll transcript preview by page (or Alt-Up/Down)\n\
        Esc             Close without printing\n\n\
        Press Enter to return to the picker.",
        legend.join("  ")
    );

    let reader = SkimItemReader::new(
        SkimItemReaderOption::default()
            .ansi(true)
            .delimiter("\t")
            .with_nth(["1"].into_iter()),
    );
    let items = reader.of_bufread(source);
    let collector = Rc::new(RefCell::new(Collector {
        reader,
        search: format!("{me} --rows{filters}{search_agent} -n {ROWS} -c --"),
    }));

    let options = SkimOptions {
        delimiter: "\t".to_string(),
        no_multi: true,
        ansi: true,
        expect: vec!["alt-p".to_string(), "alt-m".to_string()],
        // default layout draws the list bottom-up, where page-down is a no-op at the newest row
        layout: "reverse".to_string(),
        // rows arrive newest first, and that beats a fuzzy score: you are usually after a
        // session from this week, not the one whose name matches most tightly
        tiebreak: vec![RankCriteria::Index],
        // substring, like the command line. Scattered fuzzy hits turned "intercom" into 42 rows
        exact: true,
        // ctrl-q (skim's own toggle-interactive) swaps the query for this command
        cmd: Some(format!("{SEARCH}{{}}")),
        // ctrl-q then carries on from the name search instead of starting empty
        cmd_query: Some(query.to_string()),
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
        bind: [
            agents,
            vec![
                format!(
                    "?:execute[printf '\\n%s\\n' {} >/dev/tty; IFS= read -r answer </dev/tty]",
                    shell_arg(&help)
                ),
                "alt-down:preview-page-down".to_string(),
                "alt-up:preview-page-up".to_string(),
                "ctrl-down:preview-page-down".to_string(),
                "ctrl-up:preview-page-up".to_string(),
            ],
        ]
        .concat(),
        header: Some(format!(
            "Output: Enter print resume command | Alt-P print path | Alt-M export Markdown\nSearch: Ctrl-Q switch name/transcript | F1-F{} filter agent (F{} all)\nNavigation: Ctrl-Up/Down scroll preview | Esc close | ? help\n{}",
            SOURCES.len(),
            SOURCES.len() + 1,
            header_row()
        )),
        cmd_collector: collector.clone(),
        ..Default::default()
    };

    let Some(out) = Skim::run_with(&options, Some(items)) else {
        return;
    };
    if out.is_abort {
        return;
    }
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
