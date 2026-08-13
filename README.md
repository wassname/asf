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

Searching 3.4 GB of transcripts takes about a second, so there is no index to build.

## The picker

`asf -i` loads the newest 400 rows and lets skim rank them.

| key | |
|---|---|
| enter | print the resume command |
| alt-p | print the transcript path |
| ctrl-q | swap `name>` and `transcript>` search; the prompt says which you are in |
| f1..f6 | keep one agent, f7 for all of them again |
| pgdn, pgup | a page of rows |
| alt-down, alt-up | a page of the preview |

## Install

```sh
cargo install --path .
```

One binary, no runtime dependencies. It links ripgrep's crates for the search and skim's
for the picker, so there is nothing to install alongside it. Changing a dependency needs
`cargo +nightly update`, because of the 8 day publish-age hold in `.cargo/config.toml`.

`RESEARCH_JOURNAL.md` has the rest: what each agent's store looks like, what it costs, and
what has gone wrong.

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
