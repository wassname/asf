# asf - agent session finder

Find a coding-agent session by name or transcript text. Supports Claude Code, Codex,
Pi, OpenCode, Gemini, Copilot, and Hermes. Searches transcript text directly; caches session metadata.

## Install

```sh
cargo install --path . --locked
```

Put `~/.cargo/bin` on `PATH`. Hermes also needs `sqlite3`.

## Use

```sh
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

Name search reads sessions newest-first and stops at `-n` matches or an exact name
match (literal queries only). Read/preview/resume by name stops at the first match. These searches do not count older matches. IDs use
filenames first, then Pi headers when needed. Transcript search (`-c`) remains exhaustive.

Metadata is cached in `${XDG_CACHE_HOME:-~/.cache}/asf/names-v1.json`, checked against
file size and nanosecond modification time. Changed and uncached transcripts are read
again. Pi/Claude rename lookup tries the last 64 KB before a full metadata scan; names
in the middle still work. The first broad name search can be slower while the cache fills.
Delete the cache to rebuild it. <!-- PI/OpenAI -->

## Picker

The picker opens immediately and adds sessions newest first. Name search filters as rows
arrive; transcript search scans the full store before showing matches.

- `enter`: resume command; `alt-p`: transcript path; `alt-m`: markdown transcript
- `ctrl-q`: switch name/transcript search
- `f1` to `f6`: one agent; `f7`: all agents
- `pgup`/`pgdn`: list pages; `ctrl-up`/`ctrl-down`: preview pages

## Development

```sh
cargo test --locked
```

Dependency changes use `cargo +nightly update` to retain the eight-day publish-age hold.
`RESEARCH_JOURNAL.md` records store formats and implementation notes.

<!-- PI/OpenAI -->
