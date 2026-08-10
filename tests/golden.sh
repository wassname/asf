#!/usr/bin/env bash
# Record what one build of asf prints, so the other build has an oracle to match.
#
#   tests/golden.sh record python ~/.agents/skills/asf/scripts/asf
#   tests/golden.sh record rust   target/release/asf
#   tests/golden.sh diff
#
# The queries below are the ones three audits found faults with. Keep them.
set -uo pipefail
cd "$(dirname "$0")/.."

QUERIES=(
    ""                          # the newest sessions
    "steer"                     # a name that matches many
    "print(tabulate"            # a bracket: used to die as a bad regex
    "["                         # an unbalanced bracket
    "-c the"                    # a word in every transcript
    "-c a"                      # 57k opencode shards: used to break ARG_MAX
    "-c message"                # used to tear an rg record on a control byte
    "-c opencode"
    "-c steer"
    "-c  roms "
    "-c Virgin Defensive"       # a phrase nobody said: only in the pasted AGENTS.md
    "-c supply chain"
    "-c profile ships in a staging"
    "-a codex"
    "-a pi"
    "-a opencode"
    "-a gemini"
    "-a copilot"
    "--paths -c staging directory"
    "--rows -c staging directory"
)

record() {
    local name="$1" bin="$2" dir="tests/golden/$1"
    mkdir -p "$dir"
    for i in "${!QUERIES[@]}"; do
        # shellcheck disable=SC2086
        $bin ${QUERIES[$i]} -n 8 > "$dir/$(printf '%02d' "$i").txt" 2>&1
        echo "exit=$?" >> "$dir/$(printf '%02d' "$i").txt"
    done
    echo "recorded ${#QUERIES[@]} queries to $dir"
}

case "${1:-diff}" in
    record) record "$2" "$3" ;;
    diff)
        fail=0
        for f in tests/golden/python/*.txt; do
            b=tests/golden/rust/$(basename "$f")
            [ -f "$b" ] || { echo "MISSING $b"; fail=1; continue; }
            if ! diff -q "$f" "$b" >/dev/null; then
                echo "DIFFERS $(basename "$f"): ${QUERIES[$((10#$(basename "$f" .txt)))]}"
                fail=1
            fi
        done
        [ $fail -eq 0 ] && echo "all ${#QUERIES[@]} queries match"
        exit $fail
        ;;
esac
