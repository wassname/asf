# asf notes

## Session stores

| agent | store | id to resume with | name |
|---|---|---|---|
| claude | `~/.claude/projects/<cwd>/<uuid>.jsonl` | the file stem | `custom-title`, else `ai-title`, else the first message |
| codex | `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl` | the uuid in the name, = `session_meta.payload.id` | `thread_name` in `~/.codex/session_index.jsonl`, else the first message |
| pi | `~/.pi/agent/sessions/--<cwd>--/<ts>_<id>.jsonl` | the `id` of the first record | the first message |
| opencode | `~/.local/share/opencode/storage/session/<hash>/ses_*.json` | the file stem | `title` in that json |
| gemini | `~/.gemini/tmp/<project>/logs.json`, and `chats/session-*.jsonl` | none, `--resume` takes `latest` or an index | the first prompt |
| copilot | `~/.copilot/session-state/<uuid>/events.jsonl` | the directory name | the first message |
| hermes | `~/.hermes/state.db`, sqlite | `20260806_184450_5f121c02` | not read by asf |

claude, pi and opencode scope the lookup to the working directory. 82 codex and 236 opencode
sessions record a directory that no longer exists.

## Format faults

- codex writes your `AGENTS.md` into a `world_state` record in every transcript. A search for
  a phrase that only exists in that file matched 411 sessions
- claude pastes its skill list into every session as an `attachment` record
- claude files a tool result as a user turn, marked `toolUseResult`
- claude repeats the `custom-title` record, so the last one is the current name
- 320 of 549 codex rollouts are subagent runs, marked `"source":{"subagent":...}` in the header
- a pi filename truncates the id at the last underscore, and 8 of 1216 filename uuids are not
  the id pi answers to
- an opencode session is not one file. It is `storage/message/<ses>/` plus
  `storage/part/<msg>/`, and the session json holds only a title
- gemini writes some session files as `.json` and some as `.jsonl`, plus one `logs.json` of
  prompts per project
- copilot labels turns `user.message` and `assistant.message`, with no `role` key

## Search cost

2036 sessions, 3.7 GB, nvme. Each file stops at its first hit, which is what keeps a common
word from returning 200k rows.

| query | |
|---|---|
| `asf` | 0.47 s |
| `asf -c the` | 1.6 s |
| `asf -c "profile ships in a staging"` | 0.80 s |

Build a `grep_searcher::Searcher` per file, not per worker thread. A reused Searcher loses
hits in the files it searches later: `asf -c opencode` found 340 sessions instead of 341, the
same one missing every run. The cause is the line buffer, not the binary detection. Memory
maps avoid it, and measured slower here, 1.75 s against 1.05 s.

Read a header once. Testing that same record for `"subagent"` costs nothing; a second scan of
the codex store to find the same thing doubled `asf` to 0.85 s.

## skim as a library

- `reload(cmd {q})` passes the two characters `{q}` to the shell, not the query. Only
  interactive mode, `cmd` with `cmd_prompt`, substitutes it
- the default layout draws the list bottom-up from the newest row, so page-down does nothing.
  Set `layout` to `reverse`
- `SkimOptions::with_nth` is dead in library mode. The one on `SkimItemReaderOption` is what
  works, and it keeps the full line in `output()`
- there is no `change-header` action, so the mode has to show in the prompt
- `tests/drive.py` renders the picker in a pty, so a key binding can be checked without a
  person at a terminal

## sqlite

Open another agent's database with the `file:` URI form and `mode=ro`. Not `immutable=1`,
which skips the write-ahead log: codex's `state_5.sqlite` has a 4.1 MB log, and `immutable=1`
reported 537 threads where `mode=ro` reported 539.

## Regression check

```sh
tests/golden.sh record before target/release/asf   # then change something
tests/golden.sh record after  target/release/asf
tests/golden.sh diff
```

Each of the 20 queries once found a fault. Do not trim them.

-- notes by Claude
