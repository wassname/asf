# asf structured records and filters

Implemented interfaces:

```sh
asf --cwd . --paths
asf -r "$PI_INTERCOM_SESSION_ID" --role user --since 24h --tail 20
asf -r SESSION --since 2026-10-24 --until 2026-10-25
asf -r SESSION --jsonl | jq 'select(.role == "user")'
```

- Structured messages retain session, agent, CWD, UTC timestamp, role, text, and source location before rendering.
- All seven existing fixture formats have normalized JSONL and exact-time-window coverage. Pi/OpenCode numeric message times are milliseconds; Hermes database times are seconds. Other inspected message times are ISO strings.
- Date bounds are inclusive and apply to message times, not session-file modification times. Date-only bounds mean local midnight. Missing/invalid times remain null and are excluded with a count when filtering.
- Exact CWD selection precedes limits and newest-match selection, including content search, ID selection, and picker reloads. Equivalent symlink paths match; similar project names do not.
- JSONL preserves distinct source records, emits no omission markers, and succeeds on an empty selection. Markdown retains its existing adjacent-resend behavior.
- No dependency changes, embedded query interpreter, daily exporter, or human-authorship classifier.

## Verification

- `slop/final-tests.log`: 33 tests passed (21 existing functional, 1 existing name-search, 11 new filter/record tests), zero failures. After moving test caches outside fixtures, `slop/filter-tests-final.log` confirms all 11 new tests still pass.
- `slop/markdown-compatibility.log`: “Markdown compatibility: 32/32 outputs unchanged across 8 fixture sessions and 4 rendering/filter modes”.
- `slop/install.log`: release build installed at `/home/code/.cargo/bin/asf`; `asf --help` advertises all four new flags.
- `slop/installed-smoke.json`: installed CLI output piped through `jq -e`, showing Pi/user role, UTC time, and original source location. Installed `asf --cwd /home/code/.pi --paths -n 1` selected the current project transcript.

The compatibility comparison initially found that role filtering before Markdown resend collapsing merged equal user replies separated by an assistant. The order is corrected and covered by a maintained regression test. Date filtering still precedes resend collapsing, so an earlier longer message cannot hide a later shorter message inside a requested time window.

Independent static review (PI/gpt-6.1-sol, other asf session): “No blockers found in requested bounded read-only review.” It checked date units, provenance, filter-before-limit order, and JSONL validity. It did not run tests and preceded the resend-order correction; this is supporting evidence, not a substitute for the output comparison.

Real-session smoke check: debug CLI JSONL piped through `jq` and returned the latest user-role record with its source path, line, entry ID, and UTC time. Kept the actual message in `.local/recent-user-smoke.json`, not this committed report.

## Limits

`--role user` includes automated prompts. Exact CWD cannot match sessions with unknown working directories. Reads still select one session; listing paths with `--cwd` does not create a merged daily journal. Existing stored chronology and branch behavior are unchanged.

-- PI/OpenAI
