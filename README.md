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

## Project, time, and JSONL filters

```sh
asf --cwd . --paths                         # sessions from this exact directory
asf -r "$PI_INTERCOM_SESSION_ID" --role user --since 24h
asf -r <file> --since 2026-10-24 --until 2026-10-25
asf -r <file> --jsonl | jq 'select(.role == "user")'
```

`--cwd` resolves relative paths from the calling directory and matches equivalent symlink
paths, not subdirectories or similar project names. It filters before limits and newest-match
selection, including content search and picker reloads. Unknown working directories do not match.

`--since` and `--until` require `--read` and filter message times, not file modification times.
Both bounds are inclusive. Use an RFC3339 timestamp with an offset, a local date at midnight,
`now`, or a fixed duration before now (`s`, `m`, `h`, `d`, `w`). Role and time filters run
before `--head`/`--tail`. Unknown or invalid timestamps are excluded with a count on stderr.
A date-only `--until 2026-10-25` means midnight starting that day, not its end.

`--jsonl` requires `--read`. Each line has `session_id`, `agent`, `cwd`, `timestamp` (UTC
RFC3339 or null), `role`, `text`, and `source`. Source locations use a file and 1-based line
for JSONL, a 1-based array index for Gemini, message/part files for OpenCode, or a database
session and row ID for Hermes. Missing fields are null; no timestamp is inferred from file age.
Text follows the same tool/reasoning visibility flags as Markdown. JSONL retains distinct
source records even when their text repeats; Markdown still collapses adjacent resend copies.
Head/tail cuts add no omission records to JSONL. An empty JSONL selection succeeds with no output.

User-role records can be automated prompts, not human messages. The role filter does not
establish authorship.

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
