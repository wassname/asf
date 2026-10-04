# asf - agent session finder

Find a coding-agent session by name or transcript text. Supports Claude Code, Codex,
Pi, OpenCode, Gemini, Copilot, and Hermes. Uses FFF to search cached session metadata;
`-c` searches transcript text.

## Install

```sh
cargo +stable install --path . --locked
```

Put `~/.cargo/bin` on `PATH`. Hermes also needs `sqlite3`.

## Use

```sh
asf --refresh              # build or update the metadata index
asf                        # newest sessions
asf steer                  # search names, projects, and opening messages
asf -c "staging dir"       # search transcript text
asf -i steer               # pick a session; enter prints its resume command
asf --paths -c steer       # transcript paths for piping
asf -r 019ffeb2 --tail 20  # read a session as markdown
asf -p 019ffeb2            # preview directory, model, files, and first/last messages
asf -u steer               # resume command for the newest name match
```

`-r`, `-p`, and `-u` accept a transcript path, session ID, or unique ID prefix.
Exact IDs win; ambiguous prefixes fail and list the paths. Other words use name search.
Selection details go to stderr; the requested output goes to stdout.

Read your current Pi session:

```sh
asf -r "$PI_INTERCOM_SESSION_ID" --role user --tail 10
```

`--role user|assistant|tool` filters before `--head` and `--tail`. `--tools` keeps tool
calls/results; `--think` keeps reasoning. `--role tool` enables `--tools`.
`-a AGENT` limits results to one agent; in name search, it also limits which stores are
scanned. `--sub` includes subagent sessions. See `asf --help` for all flags.

Metadata results are newest-first, limited by `-n` or an exact name match (literal
queries only). Read/preview/resume selects the newest match. Exact IDs use cached
metadata first, then filename and header lookup. `-c` still scans full transcripts.

The index updates automatically. Unchanged transcripts are not opened; append-only
JSONL logs are read from their saved offset. The first indexing run can be slow.
Use `asf --refresh` to complete it before searching.

Cache: `${XDG_CACHE_HOME:-~/.cache}/asf/metadata-v2/`. After editing old records,
delete this directory and run `asf --refresh`. <!-- PI/OpenAI -->

## Picker

The picker opens immediately. After metadata refresh, results arrive newest-first.
Transcript search scans the full store before showing matches.

- `enter`: resume command; `alt-p`: transcript path; `alt-m`: markdown transcript
- `ctrl-q`: switch name/transcript search
- `f1` to `f6`: one agent; `f7`: all agents
- `pgup`/`pgdn`: list pages; `ctrl-up`/`ctrl-down`: preview pages

## Development

```sh
cargo +stable test --locked
```

Dependency changes use `cargo +nightly update` to retain the eight-day publish-age hold.
`RESEARCH_JOURNAL.md` records store formats and implementation notes.

<!-- PI/OpenAI -->
