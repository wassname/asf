---
name: asf
description: Finds and reads past coding-agent sessions. Use when asked to search sessions in a working directory, recover earlier user messages, or inspect a session by name, topic, ID, or transcript path.
---

# ASF

```sh
asf 'session name' -n 1                      # find a session by name
asf --cwd . --paths                          # list sessions recorded in this directory
asf --cwd . -c 'staging directory' --paths    # find sessions by transcript text
```

Start with name search: it matches cached metadata. Add `-c` when the words are in the conversation. `--cwd` matches the exact recorded directory, not its subdirectories, and limits which transcripts `-c` scans. `-c` returns sessions, not individual messages, and includes assistant and tool text. Raise the default limit of 20 sessions with `-n`.

Use a transcript path or session ID in place of `SESSION`:

```sh
asf -r SESSION --role user --tail 20          # recover recent user-role messages
asf -r SESSION --role user --jsonl | jq -c 'select(.text | test("staging directory"; "i"))'
asf -r SESSION --around 42                   # show record 42 and its neighbors
```

A user-role message can include automation or quoted text. Read the surrounding messages before attributing it to the user.

<!-- PI/OpenAI: public ASF skill. -->
