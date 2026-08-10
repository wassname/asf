//! The picker. skim ranks the rows it already has; ctrl-r rescans every transcript for what
//! you typed. Same split as junegunn's ripgrep and fzf modes, and the one ripgrep's author
//! argues for: a scanner searches exhaustively and does not rank, a fuzzy finder ranks and
//! does not scan gigabytes.

use crate::sessions::resume_for_path;
use skim::prelude::*;
use std::io::Cursor;

pub fn pick(tsv: String) {
    let me = std::env::current_exe()
        .expect("cannot find my own path")
        .to_string_lossy()
        .into_owned();

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
        with_nth: ["1", "2", "3", "4", "5"].map(String::from).to_vec(),
        no_multi: true,
        ansi: true,
        expect: vec!["alt-p".to_string()],
        preview: Some(format!("{me} --read {{6}} --line {{7}}")),
        preview_window: "down:65%:wrap".to_string(),
        bind: vec![
            format!("ctrl-r:reload({me} --rows -c {{q}} -n 300)"),
            format!("ctrl-n:reload({me} --rows -n 300)"),
        ],
        header: Some("enter resume   alt-p path   ctrl-r search transcripts   ctrl-n names".into()),
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
    // read the session fresh: after ctrl-r these rows are ones the caller never scanned
    println!("{}", resume_for_path(&fields[5]));
}
