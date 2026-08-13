# asf

Find a past coding-agent session by its name, or by anything said inside it. Claude Code,
Codex, pi, opencode, gemini, copilot. No index, so nothing goes stale.

```sh
asf                        # the newest sessions
asf steer                  # sessions whose NAME matches
asf -c "staging dir"       # sessions whose TRANSCRIPT matches, assistant text included
asf -i steer               # pick one; enter prints the resume command
asf --paths -c steer       # transcript paths, for piping
asf --read PATH --tail 20  # the last 20 messages, as text
asf --preview PATH         # where it ran, the files it named, its first and last words
asf --resume PATH          # the command that reopens it
```

Every row carries the transcript path, so the answer to "which session was that" is a path
you can open, not a name you have to hunt for.

```
| when       | agent  | project     | name                          | match                        | file                       |
|------------|--------|-------------|-------------------------------|------------------------------|----------------------------|
| 2026-08-09 | claude | gpu-cloud   | Fix apparmor profile staging  | the profile ships in a stag  | ~/.claude/projects/...json |
```
<img width="1173" height="180" alt="image" src="https://github.com/user-attachments/assets/64254e7f-a91f-45e4-b22b-06e890c8e8f5" />

## Install

```sh
cargo install --path .
```

One binary, no runtime dependencies. It links ripgrep's crates for the search and skim's
for the picker, so there is nothing to install alongside it.

## How it searches 3.4 GB without an index

Reading every transcript takes about a second, so there is nothing to build and nothing to
go stale. Stopping each file at its first hit is what keeps a common word from returning
200k rows; a row that turns out to be harness-pasted text gets a second, deeper look.

Measured here, against 2415 sessions:

| query | python, shelling out to rg | this |
|---|---|---|
| `asf` | 1.76 s | 0.34 s |
| `asf steer` | 1.67 s | 0.35 s |
| `asf -c the` | 3.28 s | 1.23 s |

## What the format knowledge is worth

These are the faults three audits found in real stores. They are the value of the tool, not
the code around them.

- codex stores your `AGENTS.md` in a `world_state` record in every transcript, so a search
  for a phrase that only exists in that file matched 411 sessions
- claude pastes its skill list into every session as an `attachment` record
- claude files a tool result as a user turn, marked `toolUseResult`
- claude's `/rename` writes a `custom-title` record, repeated, so the last one is the name
- a codex thread name is in `~/.codex/session_index.jsonl`, not in the rollout
- 320 of 549 codex rollouts are subagent runs: `"source":{"subagent":...}` in the header
- a pi filename truncates the id at an underscore; read the `id` of the first record
- copilot's id is the directory holding `events.jsonl`, not the file
- an opencode session is not a file: it is `storage/message/<ses>/` plus
  `storage/part/<msg>/`, and its metadata file holds only a title
- gemini writes half its session files as `.json` and half as `.jsonl`, plus one
  `logs.json` of prompts per project
- copilot labels turns `user.message` and `assistant.message`, with no `role` key
- hermes and Cursor are not supported: they keep sessions in sqlite, not in files

## The picker

`asf -i` loads the newest 400 rows and lets skim rank them. `ctrl-q` swaps the prompt from
`name>` to `transcript>`, where every keystroke rescans every transcript instead. enter
prints the resume command, alt-p the path, f1..f6 keep one agent and f7 puts them all back,
alt-up and alt-down page the preview.

Two skim traps, if you drive it as a library. `reload(cmd {q})` hands those two characters to
the shell instead of the query, so only interactive mode (`cmd` with `cmd_prompt`) can carry
one. And the default layout draws the list bottom-up from the newest row, where page-down has
nowhere to go, so `layout` has to be `reverse`. `tests/drive.py` renders the picker in a pty,
so a key binding can be checked without a person at a terminal.

## The oracle

`tests/golden.sh` records what one build prints for 20 queries and diffs two recordings. The
queries are the ones the audits found faults with, so do not trim them.

```sh
tests/golden.sh record before target/release/asf   # then change something
tests/golden.sh record after  target/release/asf
tests/golden.sh diff before after
```

Record both within minutes: the corpus grows as you work, so an older recording differs for
reasons that have nothing to do with the code. The recordings stay out of git, because they
hold real session names and paths.

The python build at `~/.agents/skills/asf/scripts/asf` was the oracle for the rust port, and
12 of the 20 queries now differ from it, all from hiding codex subagent rollouts and from
reading the names people typed. Either port those two to it or drop it; a permanently red
diff teaches nothing.

## Supply chain

`Cargo.lock` is committed, and `.cargo/config.toml` refuses any crate version published in
the last 8 days, because most compromised releases are caught within days. That is cargo's
own [min-publish-age](https://doc.rust-lang.org/nightly/cargo/reference/unstable.html#min-publish-age),
which is nightly-only for now, so resolving needs nightly and building does not:

```sh
cargo +nightly update      # writes Cargo.lock under the 8 day hold
cargo build --locked       # stable, builds exactly what the lock says
```

With the hold off, 23 of these dependencies would move today, one of them published
yesterday. Do not remove it to make an update go through: wait, or add a scoped exception
for the one crate.

## One thing that bit me

Build a `grep_searcher::Searcher` per file, not per worker thread. A reused Searcher loses
hits in the files it searches later: `asf -c opencode` found 340 sessions instead of 341,
the same one missing every run. It is the line buffer, not the binary detection. Memory
maps avoid it, which is what ripgrep does, but they measured slower here (1.75 s against
1.05 s).

## Similar tools

Finders, the ones asf competes with:

| tool | shape | why not this |
|---|---|---|
| [pratikgajjar/recall](https://github.com/pratikgajjar/recall) | Go, sqlite index, Cursor + claude + codex + pi | the nearest thing to asf, and the better tool if you want tags, cost stats and Cursor. It needs `recall index` first, and a 69.5 MiB index that `--prune` keeps honest. asf trades those features for having nothing to build |
| [subinium/agf](https://github.com/subinium/agf) | Go, fzf over sessions | searches the last message, not the name and not the transcript |
| [dmtrKovalenko/fff](https://github.com/dmtrKovalenko/fff) | Rust matcher, `fff-search` on crates.io | a library with no picker: no delimiter, preview or ANSI. Its author says it "loses on grep once from bash and exit", which is what this is |

Readers and analytics. All claude-only, and they answer "what happened in this session"
once you have its path, which is what asf hands you:

- [daaain/claude-code-log](https://github.com/daaain/claude-code-log), transcript jsonl to HTML or markdown
- [simonw/claude-code-transcripts](https://github.com/simonw/claude-code-transcripts), publish a session as a page
- [vtemian/claude-notes](https://github.com/vtemian/claude-notes), the same for a terminal
- [Alfredvc/cct](https://github.com/Alfredvc/cct), transcripts as SQL over DuckDB
- [spences10/ccrecall](https://github.com/spences10/ccrecall), syncs transcripts into sqlite for analytics
- [ysamlan/agent-log-gif](https://github.com/ysamlan/agent-log-gif), transcripts as animated gifs

Compaction is the other neighbour: [pi-vcc](https://github.com/sting8k/pi-vcc) shortens a
live pi session by extraction rather than by asking a model, after
[lllyasviel/VCC](https://github.com/lllyasviel/VCC). asf reads finished sessions, so the two
do not overlap, but the head, tail and files layout of `asf --preview` is the same idea.

If you add a sqlite source, copy recall's care, not just its schema. Its
[cursor.go](https://github.com/pratikgajjar/recall/blob/main/cursor.go) warns that a bare
`path?params` DSN "is silently opened read-write and CHECKPOINTS a WAL database on close,
mutating the user's source data". Use the `file:` URI form with `mode=ro`, and not
`immutable=1` on a live agent's database: codex's `state_5.sqlite` carries a 4.1 MB
write-ahead log here, and `immutable=1` skips it, reporting 537 threads where `mode=ro`
reported 539.
