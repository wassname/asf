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

one() {  # name bin index
    local file="tests/golden/$1/$(printf '%02d' "$3").txt"
    mkdir -p "tests/golden/$1"
    # shellcheck disable=SC2086
    $2 ${QUERIES[$3]} -n 8 > "$file" 2>&1
    echo "exit=$?" >> "$file"
}

record() {
    local name="$1" bin="$2"
    for i in "${!QUERIES[@]}"; do one "$name" "$bin" "$i"; done
    echo "recorded ${#QUERIES[@]} queries to tests/golden/$name"
}

# Record both builds per query, seconds apart. A live session gets written while this runs,
# so two full passes drift: rows move and counts differ for reasons that are not the code.
both() {
    for i in "${!QUERIES[@]}"; do
        one python "$1" "$i"
        one rust "$2" "$i"
    done
    echo "recorded ${#QUERIES[@]} queries for both builds"
}

case "${1:-diff}" in
    record) record "$2" "$3" ;;
    both) both "$2" "$3" ;;
    diff)
        fail=0
        for f in tests/golden/python/*.txt; do
            b=tests/golden/rust/$(basename "$f")
            [ -f "$b" ] || { echo "MISSING $b"; fail=1; continue; }
            # sorted: a session written while this runs moves rank between the two reads,
            # so compare the set of rows, not their order
            if ! diff -q <(sort "$f") <(sort "$b") >/dev/null; then
                echo "DIFFERS $(basename "$f"): ${QUERIES[$((10#$(basename "$f" .txt)))]}"
                fail=1
            fi
        done
        [ $fail -eq 0 ] && echo "all ${#QUERIES[@]} queries match"
        exit $fail
        ;;
esac
