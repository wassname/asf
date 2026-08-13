//! The search, using ripgrep's own crates. `max_count` stops each file at its first hit,
//! which is what keeps a common word from returning 200k rows.

use grep_regex::RegexMatcherBuilder;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkMatch};
use ignore::WalkBuilder;
use ignore::overrides::{Override, OverrideBuilder};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct Hit {
    pub line: u64,
    pub text: String,
}

pub type Hits = BTreeMap<String, Vec<Hit>>;

pub struct Scan<'a> {
    pub literal: bool,
    pub icase: bool,
    pub globs: &'a [&'a str],
    pub max_count: usize,
}

/// Only these files are sessions. The stores also hold hook stdout, notes and caches.
pub const JSONISH: [&str; 1] = ["*.json*"];

/// The opencode excludes are its own bookkeeping, not conversation.
pub const CONTENT_GLOBS: [&str; 10] = [
    "*.json*",
    "!**/session_diff/**",
    "!**/session_share/**",
    "!**/plugin/**",
    "!**/todo/**",
    "!**/checkpoint-*",
    "!**/tool-results/**",
    "!**/rewind-snapshots/**",
    "!*index.json",
    "!vscode.metadata.json",
];

fn globs(patterns: &[&str]) -> Override {
    let mut builder = OverrideBuilder::new("/");
    for pattern in patterns {
        builder.add(pattern).expect("bad glob");
    }
    builder.build().expect("bad glob set")
}

struct Collect {
    max: usize,
    seen: usize,
    hits: Vec<Hit>,
}

impl Sink for Collect {
    type Error = std::io::Error;

    fn matched(&mut self, _: &Searcher, m: &SinkMatch<'_>) -> Result<bool, std::io::Error> {
        self.seen += 1;
        // a transcript can hold a non-utf8 byte inside a string; ripgrep reports such a line
        // as bytes and asf skips it, but it still counts against max_count
        if let Ok(text) = std::str::from_utf8(m.bytes()) {
            self.hits.push(Hit {
                line: m.line_number().unwrap_or(0),
                text: text.trim_end_matches('\n').trim_end_matches('\r').to_string(),
            });
        }
        Ok(self.seen < self.max)
    }
}

/// First `max_count` matching lines per file, keyed by path.
pub fn search(pattern: &str, paths: &[PathBuf], scan: &Scan) -> Hits {
    let over = globs(scan.globs);
    let (dirs, files): (Vec<_>, Vec<_>) = paths
        .iter()
        .filter(|p| p.exists())
        .cloned()
        .partition(|p| p.is_dir());
    // ignore's walk does not filter the roots it was given, so an explicitly named file
    // gets the glob test here instead
    let roots: Vec<PathBuf> = dirs
        .into_iter()
        .chain(
            files
                .into_iter()
                .filter(|p| !over.matched(p, false).is_ignore()),
        )
        .collect();
    if roots.is_empty() {
        return Hits::new();
    }

    let wanted = if scan.literal {
        regex::escape(pattern)
    } else {
        pattern.to_string()
    };
    let matcher = match RegexMatcherBuilder::new()
        .case_insensitive(scan.icase)
        .line_terminator(Some(b'\n'))
        .build(&wanted)
    {
        Ok(matcher) => matcher,
        Err(err) => {
            eprintln!("asf: bad pattern {pattern:?}: {err}");
            std::process::exit(1);
        }
    };

    let mut walk = WalkBuilder::new(&roots[0]);
    for root in &roots[1..] {
        walk.add(root);
    }
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    walk.overrides(over).threads(threads.min(12));

    let found: Mutex<Hits> = Mutex::new(Hits::new());
    let hits = &found;
    walk.build_parallel().run(|| {
        let matcher = matcher.clone();
        Box::new(move |entry| {
            // A reused Searcher loses later-file hits because its line buffer carries state.
            let mut searcher = SearcherBuilder::new()
                .binary_detection(BinaryDetection::quit(0))
                .line_number(true)
                .build();
            let Ok(entry) = entry else {
                return ignore::WalkState::Continue; // unreadable file, as rg --no-messages
            };
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                return ignore::WalkState::Continue;
            }
            let mut sink = Collect { max: scan.max_count, seen: 0, hits: Vec::new() };
            if searcher.search_path(&matcher, entry.path(), &mut sink).is_ok()
                && !sink.hits.is_empty()
            {
                hits.lock()
                    .unwrap()
                    .insert(entry.path().to_string_lossy().into_owned(), sink.hits);
            }
            ignore::WalkState::Continue
        })
    });
    found.into_inner().unwrap()
}

/// Every `ses_*.json` under a directory, or the file itself.
pub fn files_under(root: &Path, prefix: &str) -> Vec<PathBuf> {
    if !root.exists() {
        return Vec::new();
    }
    WalkBuilder::new(root)
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .filter(|e| {
            e.path()
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(prefix) && n.ends_with(".json"))
        })
        .map(|e| e.into_path())
        .collect()
}
