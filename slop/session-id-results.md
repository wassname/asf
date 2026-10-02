# Session ID lookup fix

`-r`, `-p`, and `-u` now look up IDs before searching transcript names. Lookup lists files only at each agent's session-file depth. It verifies Pi IDs against headers and checks other Pi headers when a filename lookup is insufficient. Exact IDs beat prefixes; ambiguous prefixes and missing ID-shaped inputs fail explicitly. Names still use the existing search.

The transcript readers and normal search remain unchanged. The current installed binary is `/home/code/.cargo/bin/asf`.

## Verification

`cargo test --locked`, from `slop/session-id-tests.log`:

> test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.61s

The maintained tests cover full IDs and prefixes for all six resumable agents, all three action flags, Pi filename/header disagreement, ambiguous prefixes including header-only matches, exact-match priority, agent/subagent filters, missing IDs, and the existing read/name/content behavior.

Real-store check, from `slop/session-id-verification.log`:

```text
id lookup (with strace): elapsed=0.16 exit=0
path read (without strace): elapsed=0.01 exit=0
stdout matches direct path read: yes
distinct transcript files opened: 1 (the requested file)
subagent-artifact directories opened: 0
```

Without strace, `slop/session-id-untraced.log` records:

> elapsed=0.08 exit=0

Before the fix, the installed binary's same ID command timed out. `.local/session-id-before.time` records:

```text
Command exited with non-zero status 124
elapsed=8.03 exit=124
```

These timings are single observations on the current transcript, not the earlier reported 92 MB transcript. Raw traces and transcript text stay in ignored `.local/`.

Interpretation: matching the direct-path output and opening only the requested transcript rules out an empty-result shortcut or a full transcript scan for this measured ID. Fixture tests alone were insufficient: an initial implementation passed them but still traversed the real store's artifact directories. The final real-store check also excludes that traversal.

## Recheck

```sh
cd /home/code/dev/asf
cargo test --locked
bash slop/verify_session_id_lookup.sh
asf -r "$PI_INTERCOM_SESSION_ID" --role user --tail 10
```

The verification script requires `strace`, GNU `time`, and the current Pi session ID. Installation used `cargo install --path . --locked --offline --force`. No dependencies or supply-chain settings changed.

-- PI/OpenAI
