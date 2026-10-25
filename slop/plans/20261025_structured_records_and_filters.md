# Structured transcript records and useful filters

User: “the generally useful stuff can be done” (spelling corrected), approving structured records, JSONL output, date filters, and exact-CWD selection.

- [x] Normalize supported formats before rendering
  - Keep role, text, message timestamp, session ID, agent, CWD, and original record location.
  - Preserve existing Markdown and tool/reasoning visibility.
  - Failure mode: a nested tool timestamp replaces the message time, or Gemini/OpenCode/Hermes lose provenance.
  - Deliverable: normalized JSONL from all seven fixture formats, compared with existing Markdown tests.
- [x] Add message-time and project filters
  - `--since` / `--until` require `--read`; accept RFC3339, local calendar dates, or relative durations such as `24h`.
  - Bounds are inclusive; apply role/time filters before head/tail. Exclude and count unknown timestamps when time filtering.
  - `--cwd PATH` selects an exact normalized directory before limits or newest-match selection, including content search and picker reloads.
  - Failure mode: filtering after a limit hides an older matching project, or file mtime admits an undated message.
  - Deliverable: CLI regressions exercising boundaries, missing times, cross-format time fields, and project mismatches.
- [x] Verify and install the local CLI
  - Run existing tests plus new regressions; compare old/new Markdown on fixtures and sample JSONL/date filtering on a real session.
  - Update README and the existing asf skill; retain source attribution and distinguish user-role from human authorship.
  - No daily-journal helper, embedded jq, or dependency changes.

## UAT / Verification

- `cargo +stable test --locked`: old behavior and new cross-format tests pass.
- JSONL pipes to `jq` without Markdown omission markers or diagnostics on stdout.
- Wrong-project/newer fixtures cannot hide matching-project/older fixtures under `-n 1`.
- Unknown timestamps remain null in JSONL and are reported/excluded by time filters.
- Save verification output and example commands in `slop/results.md`; install with `cargo +stable install --path . --locked`.

-- PI/OpenAI
