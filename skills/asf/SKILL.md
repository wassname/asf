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

`--cwd` matches the exact recorded working directory, not its subdirectories. The default limit is 20 sessions; raise it with `-n`. `-c` returns sessions, not individual messages, and searches assistant and tool text too.

Use a transcript path or session ID in place of `SESSION`:

```sh
asf -r SESSION --role user --tail 20          # recover recent user-role messages
asf -r SESSION --role user --jsonl | jq -c 'select(.text | test("staging directory"; "i"))'
asf -r SESSION --around 42                   # show record 42 and its neighbors
```

A user-role message can include automation or quoted text. Read the surrounding messages before attributing it to the user.

<!-- PI/OpenAI: public ASF skill. -->
