# asf - agent session finder

Find a past coding-agent session by its name, or by anything said inside it. Claude Code,
Codex, Pi, OpenCode, Gemini, Copilot, Hermes, and Antigravity.

![asf picker: searching example sessions for timeout, with a preview of the selected session](docs/assets/picker.png)

Run `asf -i` to pick a session. Type to search by name, `Ctrl-Q` to search inside
sessions, and `Enter` to print the command that reopens the selected session.
Press `?` for picker keys or run `asf --help` for command-line options.

## For agents

The [ASF skill](https://github.com/wassname/skills/blob/main/skills/asf/SKILL.md)
(private repository) shows how to find and read past sessions. Use ASF’s ripgrep-powered
search to find transcripts that match a phrase and were recorded in this exact working directory:

```sh
asf --cwd . -c 'staging directory' --paths
```

<!-- PI/OpenAI: skill link and working-directory search example. -->

## Install

From this checkout:

```sh
cargo +stable install --path . --locked
```

Put `~/.cargo/bin` on `PATH`. Hermes and Antigravity need `sqlite3`.

[Research journal](RESEARCH_JOURNAL.md)

<!-- PI/OpenAI: demo and install instructions. -->
