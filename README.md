# asf

Find a past coding-agent session by its name, or by anything said inside it. Claude Code,
Codex, pi, opencode, gemini, copilot. No index, so nothing goes stale.

Status: porting from python to rust. The python version works and is in daily use at
`~/.agents/skills/asf/scripts/asf`. This repo is the rust port.

## Why a port

The python version shells out to ripgrep and parses its json back. That boundary hid two
real bugs: ripgrep's exit code 2 was discarded, so a query containing `(` printed
"nothing matched", and python's `splitlines()` tore a ripgrep record in half on a control
byte inside a pdf that opencode had stored. Linking ripgrep's own crates makes those
errors typed instead of textual, and gives one binary to install.

| job | crate |
|---|---|
| walk the session directories | `ignore` |
| search | `grep-searcher`, `grep-regex` |
| read records | `serde_json` |
| the picker | `skim` |

## The port has an oracle, use it

`tests/golden.sh` records what one build prints for 20 queries, and diffs two builds. The
queries are the ones three audits found faults with; do not trim them.

```sh
tests/golden.sh record python ~/.agents/skills/asf/scripts/asf
cargo build --release
tests/golden.sh record rust target/release/asf
tests/golden.sh diff
```

Record both on the same day: the corpus grows, so golden output from last week will differ
for reasons that have nothing to do with the port.

## What the port must not lose

These are the format faults three audits found. They are the value of the tool, not the
code around them.

- codex stores your `AGENTS.md` in a `world_state` record in every transcript, so a
  search for a phrase that only exists in that file matched 411 sessions
- claude pastes its skill list into every session as an `attachment` record
- claude files a tool result as a user turn, marked `toolUseResult`
- an opencode session is not a file: it is `storage/message/<ses>/` plus
  `storage/part/<msg>/`, and its metadata file holds only a title
- gemini writes half its session files as `.json` and half as `.jsonl`, plus one
  `logs.json` of prompts per project
- copilot labels turns `user.message` and `assistant.message`, with no `role` key
- hermes is not supported: its sessions live in `~/.hermes/state.db`, a sqlite file
- an rg hit list can be 57k paths, over `ARG_MAX` for one argv
