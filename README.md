# asf - agent session finder

Find a past coding-agent session by its name, or by anything said inside it. Claude Code,
Codex, pi, opencode, gemini, copilot, hermes. No index, so nothing goes stale.

```sh
asf                        # the newest sessions
asf steer                  # sessions whose NAME matches
asf -c "staging dir"       # sessions whose TRANSCRIPT matches, assistant text included
asf -i steer               # pick one; enter prints the resume command
asf --paths -c steer       # transcript paths, for piping
asf -r 019ffeb2            # that session as markdown, by the id its resume command shows
asf -r 019ffeb2 --tail 20  # its last 20 messages; --tools --think keep those too
asf -p 019ffeb2            # where it ran, its model, the files it named, first and last words
asf -u steer               # the command that reopens the newest session named steer
```

Read your current Pi transcript when you need exact earlier wording, decisions, or context:

```sh
asf -r "$PI_INTERCOM_SESSION_ID" --tail 40
```

`-r`, `-p`, and `-u` are short forms of `--read`, `--preview`, and `--resume`. `--role
user|assistant|tool` filters a read before `--head` and `--tail`. `--role tool` enables
`--tools` and selects tool records; `--tools` also retains tool details inside user and assistant
records.

`-r`, `-p`, and `-u` resolve session IDs from filenames before searching transcript names.
Pi IDs are verified against their headers; when filenames differ, lookup reads only headers.
An exact ID beats a prefix match. Ambiguous prefixes fail and list the paths; missing IDs fail
without a name scan. Other words still use name search. <!-- PI/OpenAI -->

When a query resolves `-r`, `-p`, or `-u`, asf prints its choice and up to two next matches on
stderr. The transcript, preview, or resume command stays on stdout. No matching sessions fail
explicitly. Stores for agents you do not have are ignored.

Every row carries the transcript path, so the answer to "which session was that" is a path
you can open, not a name you have to hunt for.

```
| when       | agent  | project     | name                          | match                        | file                       |
|------------|--------|-------------|-------------------------------|------------------------------|----------------------------|
| 2026-08-09 | claude | gpu-cloud   | Fix apparmor profile staging  | the profile ships in a stag  | ~/.claude/projects/...json |
```
<img width="1173" height="180" alt="image" src="https://github.com/user-attachments/assets/64254e7f-a91f-45e4-b22b-06e890c8e8f5" />

It searches with ripgrep's crates instead of an index. The earlier half-second measurement
was on 3.7 GB of transcripts; a complete scan now takes much longer on a 13 GB Pi store.

## The picker

`asf -i` opens immediately and adds sessions newest first as it scans. You can select a
recent session without waiting for older transcripts. Name queries filter as rows arrive.
`ctrl-q` transcript search still scans the whole store before it shows matches.

| key | |
|---|---|
| enter | print the resume command |
| alt-p | print the transcript path |
| alt-m | print the whole session as markdown, same as `--read` |
| ctrl-q | swap `name>` and `transcript>` search; the prompt says which you are in |
| f1..f6 | keep one agent, f7 for all of them again |
| pgdn, pgup | a page of rows |
| ctrl-down, ctrl-up (alt too) | a page of the preview |

The preview shows its first and last two messages, with an omitted-message divider between.
It uses `bat` to colour the transcript Markdown when `bat` is on `PATH`; otherwise it stays plain text.

## Install

```sh
cargo install --path .
```

Cargo installs `asf` in `~/.cargo/bin`. Put that directory on `PATH` before running it:

```sh
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

One binary. It uses ripgrep's crates to search and skim's for the picker. A dependency
change needs `cargo +nightly update`, for the 8 day publish-age hold in `.cargo/config.toml`.

`RESEARCH_JOURNAL.md` has the store layouts, the search cost and the skim notes.

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

<!-- PI[gpt-5.6-terra]: updated CLI examples and self-reading guidance. -->
