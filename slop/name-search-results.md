# Session name search speed

User: “first search the session id (filename), then the metadata in the head and tail, then the full” and “search from recent to old”.

Observed cause:
- Default CLI name search loaded metadata from every store before applying `-a`, sorting by time, or applying `-n`.
- Finding Pi/Claude rename records could read complete transcripts.
- The skill told agents to “start here” with `asf -c`, which explicitly scans full transcripts.

Changes:
- Session IDs use filename lookup, with Pi header checks when needed.
- Name search uses recent-first processing, filters the agent before scanning, and stops at the limit or an exact literal name. Read/preview/resume stops at the newest match.
- Metadata cache uses file size and nanosecond modification time. Changed files are read again; deleted files are not returned.
- Opening-message lookup stops at the first real prompt. Pi/Claude rename lookup checks the last 64 KB before scanning the full file. This preserves names buried in the middle and Claude's title priority.
- Session enumeration skips nested Pi tool artifacts. Codex's separate name index is read afresh per invocation.
- Skill instructions now try a known name/ID before `-c`.

Measurements on this machine, query `Commit and push cleanup`, agent Pi:
- Old installed command: still running at 303 seconds, no result; `/proc/44416/io` reported `rchar: 15660079967`. Cancelled. Source: `slop/name-search-before.log`.
- Patched release binary, metadata cache initially absent: `elapsed_seconds=3.94 max_rss_kb=15100`. Source: `slop/name-search-after-cold.log`.
- Repeat: `elapsed_seconds=0.41 max_rss_kb=7524`. Source: `slop/name-search-after-warm.log`.
- Final installed binary, default limit (no `-n 1`): `elapsed_seconds=5.36 max_rss_kb=8924`; repeat `elapsed_seconds=1.26 max_rss_kb=8572`, both with “search stopped at an exact name”. Sources: `slop/name-search-after-installed.log` and `slop/name-search-after-installed-repeat.log`.
- All patched runs returned session `01a0fca1-78e0-7597-ac3e-9d4dbcfddc7b`, name `Commit and push cleanup`.
- A subsequent `strace` run returned the same path without opening that transcript. Sources: `slop/name-search-cache-proof.log` and `slop/name-search-cache-open.log`. This separates metadata-cache use from filesystem-cache speed.

These are observations, not a controlled speed ratio. The disk was busy, the baseline did not finish, and its reads may have warmed the filesystem cache. “Cold” here means the metadata cache, not the filesystem cache.

Verification: final `cargo test --locked` passed all 22 tests; `cargo build --release --locked` and `git diff --check` passed. The release binary was installed at `/home/code/.cargo/bin/asf`. Sources: `slop/name-search-tests.log` and `slop/name-search-build.log`. New regression checks recent-first stopping, exact-name stopping without `-n 1`, cache refresh after renaming, names outside both end windows, deletion, Codex index-only rename, and Claude title priority.

Limitations: an uncached or changed session, including a newer session before the wanted match, can still need a full metadata scan. A first broad search or a no-match query can inspect many such sessions. Full transcript search remains explicitly `-c` and scans all stores; this patch does not change that mode.

-- PI/OpenAI
