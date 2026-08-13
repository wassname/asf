# asf research journal

What the stores actually look like, and what went wrong. Notes to whoever changes this next.

## 2026-08-11, the rust port

Reading every transcript takes about a second, so there is nothing to build and nothing to
go stale. Stopping each file at its first hit is what keeps a common word from returning
200k rows; a row that turns out to be harness-pasted text gets a second, deeper look.

Measured against 2415 sessions:

| query | python, shelling out to rg | rust |
|---|---|---|
| `asf` | 1.76 s | 0.34 s |
| `asf steer` | 1.67 s | 0.35 s |
| `asf -c the` | 3.28 s | 1.23 s |

Build a `grep_searcher::Searcher` per file, not per worker thread. A reused Searcher loses
hits in the files it searches later: `asf -c opencode` found 340 sessions instead of 341,
the same one missing every run. It is the line buffer, not the binary detection. Memory
maps avoid it, which is what ripgrep does, but they measured slower here (1.75 s against
1.05 s).

`Cargo.lock` is committed, and `.cargo/config.toml` refuses any crate version published in
the last 8 days. That is cargo's own
[min-publish-age](https://doc.rust-lang.org/nightly/cargo/reference/unstable.html#min-publish-age),
nightly-only for now, so `cargo +nightly update` resolves and `cargo build --locked` builds.
With the hold off, 23 of these dependencies would move today, one of them published
yesterday. Do not remove it to make an update go through: wait, or add a scoped exception
for the one crate.

## What the format knowledge is worth

The faults audits found in real stores. They are the value of the tool, not the code around
them.

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

## 2026-08-13, driving skim as a library

`reload(cmd {q})` hands those two characters to the shell instead of the query, so only
interactive mode (`cmd` with `cmd_prompt`) can carry one. The default layout draws the list
bottom-up from the newest row, where page-down has nowhere to go, so `layout` has to be
`reverse`. `tests/drive.py` renders the picker in a pty, so a key binding can be checked
without a person at a terminal.

## Reading another agent's sqlite

If you add a sqlite source, copy [recall](https://github.com/pratikgajjar/recall)'s care, not
just its schema. Its [cursor.go](https://github.com/pratikgajjar/recall/blob/main/cursor.go)
warns that a bare `path?params` DSN "is silently opened read-write and CHECKPOINTS a WAL
database on close, mutating the user's source data". Use the `file:` URI form with `mode=ro`,
and not `immutable=1` on a live agent's database: codex's `state_5.sqlite` carries a 4.1 MB
write-ahead log here, and `immutable=1` skips it, reporting 537 threads where `mode=ro`
reported 539.

## The golden queries

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

Open question. The python build at `~/.agents/skills/asf/scripts/asf` was the oracle for the
rust port, and 12 of the 20 queries now differ from it, all from hiding codex subagent
rollouts and from reading the names people typed. Either port those two to it or drop it.

-- Claude
