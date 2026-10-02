#!/usr/bin/env bash
# PI/OpenAI: check real-store lookup; keep transcript text and raw traces in .local/.
set -euo pipefail
cd "$(dirname "$0")/.."
binary=${1:-asf}
: "${PI_INTERCOM_SESSION_ID:?set the current Pi session id}"
shopt -s nullglob
paths=("$HOME"/.pi/agent/sessions/*/*"$PI_INTERCOM_SESSION_ID".jsonl)
if (( ${#paths[@]} != 1 )); then
    echo "expected one transcript path, found ${#paths[@]}" >&2
    exit 1
fi
path=${paths[0]}
mkdir -p .local
/usr/bin/time -f 'elapsed=%e exit=%x' -o .local/session-id-verified.time \
    timeout 10s strace -f -e trace=openat -o .local/session-id-verified.strace \
    "$binary" -r "$PI_INTERCOM_SESSION_ID" --role user --tail 10 \
    >.local/session-id-read.md 2>.local/session-id-verified.stderr
/usr/bin/time -f 'elapsed=%e exit=%x' -o .local/session-path-verified.time \
    timeout 10s "$binary" -r "$path" --role user --tail 10 \
    >.local/session-path-read.md 2>.local/session-path-verified.stderr
cmp .local/session-id-read.md .local/session-path-read.md
grep -oE '"[^"]+\.jsonl"' .local/session-id-verified.strace | sort -u >.local/opened-transcripts.txt
printf '"%s"\n' "$path" | cmp - .local/opened-transcripts.txt
if grep -q 'subagent-artifacts' .local/session-id-verified.strace; then
    echo "ID lookup entered subagent artifacts" >&2
    exit 1
fi
printf 'id lookup (with strace): '; cat .local/session-id-verified.time
printf 'path read (without strace): '; cat .local/session-path-verified.time
printf 'stdout matches direct path read: yes\n'
printf 'distinct transcript files opened: 1 (the requested file)\n'
printf 'subagent-artifact directories opened: 0\n'
