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
- an opencode session is not a file: it is `storage/message/<ses>/` plus
  `storage/part/<msg>/`, and its metadata file holds only a title
- gemini writes half its session files as `.json` and half as `.jsonl`, plus one
  `logs.json` of prompts per project
- copilot labels turns `user.message` and `assistant.message`, with no `role` key
- hermes and Cursor are not supported: they keep sessions in sqlite, not in files

## Prior art

| tool | shape | why not this |
|---|---|---|
| [pratikgajjar/recall](https://github.com/pratikgajjar/recall) | Go, sqlite index, Cursor + claude + codex + pi | the nearest thing to asf, and the better tool if you want tags, cost stats and Cursor. It needs `recall index` first, and a 69.5 MiB index that `--prune` keeps honest. asf trades those features for having nothing to build |
| [subinium/agf](https://github.com/subinium/agf) | Go, fzf over sessions | searches the last message, not the name and not the transcript |
| [dmtrKovalenko/fff](https://github.com/dmtrKovalenko/fff) | Rust matcher, `fff-search` on crates.io | a library with no picker: no delimiter, preview or ANSI. Its author says it "loses on grep once from bash and exit", which is what this is |

If you add a sqlite source, copy recall's care, not just its schema. Its
[cursor.go](https://github.com/pratikgajjar/recall/blob/main/cursor.go) warns that a bare
`path?params` DSN "is silently opened read-write and CHECKPOINTS a WAL database on close,
mutating the user's source data". Use the `file:` URI form with `mode=ro&immutable=1`.

## The oracle

A python build of the same tool is the reference. `tests/golden.sh` records what one build
prints for 20 queries and diffs two builds; the queries are the ones the audits found faults
with, so do not trim them.

```sh
tests/golden.sh record python ~/.agents/skills/asf/scripts/asf
cargo build --release
tests/golden.sh record rust target/release/asf
tests/golden.sh diff        # all 20 queries match
```

Record both on the same day: the corpus grows, so yesterday's recording differs for reasons
that have nothing to do with the code. The recordings stay out of git, because they hold
real session names and paths.

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
