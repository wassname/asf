//! The picker. skim ranks the rows it already has; ctrl-q switches the query to a command
//! that rescans every transcript. Same split as junegunn's ripgrep and fzf modes, and the one
//! ripgrep's author argues for: a scanner searches exhaustively and does not rank, a fuzzy
//! finder ranks and does not scan gigabytes.

use crate::sessions::{SOURCES, resume_for_path};
use skim::prelude::*;
use std::io::Cursor;

pub fn pick(tsv: String, filters: &str, query: &str) {
    let me = std::env::current_exe()
        .expect("cannot find my own path")
        .to_string_lossy()
        .into_owned();

    // f1..f6 keep one agent, f7 puts them all back. skim has no alt-digit key, and a reload
    // keeps whatever you have typed.
    let mut agents = Vec::new();
    let mut legend = Vec::new();
    for (n, (agent, _)) in SOURCES.iter().enumerate() {
        agents.push(format!("f{}:reload({me} --rows{filters} -a {agent} -n 300)", n + 1));
        legend.push(format!("f{} {agent}", n + 1));
    }
    agents.push(format!("f{}:reload({me} --rows{filters} -n 300)", SOURCES.len() + 1));
    legend.push(format!("f{} all", SOURCES.len() + 1));

    let reader = SkimItemReader::new(
        SkimItemReaderOption::default()
            .ansi(true)
            .delimiter("\t")
            .with_nth(["1", "2", "3", "4", "5"].into_iter()),
    );
    let collector = Rc::new(RefCell::new(reader));
    let items = collector.borrow().of_bufread(Cursor::new(tsv));

    let options = SkimOptions {
        delimiter: "\t".to_string(),
        no_multi: true,
        ansi: true,
        expect: vec!["alt-p".to_string()],
        // default layout draws the list bottom-up, where page-down is a no-op at the newest row
        layout: "reverse".to_string(),
        // ctrl-q (skim's own toggle-interactive) swaps the query for this command
        cmd: Some(format!("{me} --rows{filters} -n 300 -c '{{}}'")),
        // ctrl-q then carries on from the name search instead of starting empty
        cmd_query: Some(query.to_string()),
        prompt: "name> ".to_string(),
        cmd_prompt: "transcript> ".to_string(),
        preview: Some(format!("{me} --preview {{6}} --line {{7}}")),
        preview_window: "down:65%:wrap".to_string(),
        bind: [
            agents,
            vec![
                "alt-down:preview-page-down".to_string(),
                "alt-up:preview-page-up".to_string(),
            ],
        ]
        .concat(),
        header: Some(format!(
            "enter resume  alt-p path  ctrl-q name<->transcript  alt-up/down preview\n{}",
            legend.join("  ")
        )),
        cmd_collector: collector.clone(),
        ..Default::default()
    };

    let Some(out) = Skim::run_with(&options, Some(items)) else { return };
    if out.is_abort {
        return;
    }
    let Some(chosen) = out.selected_items.first() else { return };
    let fields: Vec<String> = chosen.output().split('\t').map(str::to_string).collect();
    if fields.len() < 6 {
        return;
    }
    if out.final_key == Key::Alt('p') {
        println!("{}", fields[5]);
        return;
    }
    // read the session fresh: after a transcript search these rows are ones the caller never scanned
    println!("{}", resume_for_path(&fields[5]));
}
