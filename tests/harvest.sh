#!/usr/bin/env bash
# Rebuild tests/fixtures/home from the real sessions on this machine.
#
# Records keep their keys, types, roles and ids, which is what asf parses. Every free text
# value is replaced, so no private text is committed. Run this when an agent changes format.
set -euo pipefail
cd "$(dirname "$0")/.."
asf=${ASF:-target/release/asf}
out=tests/fixtures/home
repo=/tmp/asf-fixture-repo

scrub() { # $1 agent; jsonl on stdin
  jq -c --arg agent "$1" '
    def keep: ["type","role","name","tool","toolName","tool_name","id","uuid","call_id",
               "toolCallId","tool_call_id","session_id","sessionId","sessionID","parentUuid",
               "model","provider","version","timestamp","status","source","namespace",
               "finish_reason","messageID","callID","state","kind","projectHash"];
    def titles: ["aiTitle","customTitle","agentName","title","thread_name","display_name"];
    def filler: "the \($agent) widget factory ships on tuesday";
    def fix:
      if type == "object" then
        with_entries(.value =
          (if (.value | type) == "string" then
            if (.key | IN(titles[])) then "fixture \($agent) \(.key)"
            elif (.key | IN("cwd","workdir","git_repo_root","directory")) then "/tmp/asf-fixture-repo"
            elif (.key | IN(keep[])) then .value
            else filler
            end
          else (.value | fix) end))
      elif type == "array" then map(if type == "string" then filler else fix end)
      else . end;
    fix' 2>/dev/null
}

take() { # $1 agent, $2 source file, $3 dest, $4 lines, or "all" for one whole json
  mkdir -p "$(dirname "$3")"
  if [ "$4" = all ]; then cat "$2"; else head -n "$4" "$2"; fi | scrub "$1" >"$3"
}

rm -r "$out" 2>/dev/null || true
mkdir -p "$out"

# claude: the name records live at the end of the file, so take the last of each kind too
src=$($asf --paths -a claude -n 1)
dst=$out/.claude/projects/-tmp-asf-fixture-repo/$(basename "$src")
take claude "$src" "$dst" 30
for kind in ai-title custom-title agent-name; do
  { grep "\"type\":\"$kind\"" "$src" || true; } | tail -1 | scrub claude >>"$dst"
done

# codex: one real session, and one subagent run that must stay hidden
src=$($asf --paths -a codex -n 1)
take codex "$src" "$out/.codex/sessions/2026/08/15/$(basename "$src")" 30
src=$($asf --sub --paths -a codex -n 40 | xargs grep -l '"subagent"' | sed -n 1p)
take codex "$src" "$out/.codex/sessions/2026/08/15/$(basename "$src")" 12

# pi: one session it minted a uuid for, and one a tool named, which must stay hidden
src=$($asf --paths -a pi -n 1)
take pi "$src" "$out/.pi/agent/sessions/--tmp-asf-fixture-repo--/$(basename "$src")" 30
src=$($asf --sub --paths -a pi -n 200 | grep -E '_(rev|oracle|panel)-' | sed -n 1p)
take pi "$src" "$out/.pi/agent/sessions/--tmp-asf-fixture-repo--/$(basename "$src")" 12

# copilot
src=$($asf --paths -a copilot -n 1)
take copilot "$src" "$out/.copilot/session-state/$(basename "$(dirname "$src")")/events.jsonl" 30

# gemini: logs.json is one array for the whole project, not jsonl
src=$($asf --paths -a gemini -n 1)
dst=$out/.gemini/tmp/$(basename "$(dirname "$src")")/logs.json
mkdir -p "$(dirname "$dst")"
jq -c '.[0:6]' "$src" | scrub gemini >"$dst"

# opencode: a session is its json plus a directory of messages and one of parts each
src=$($asf --paths -a opencode -n 1)
ses=$(basename "$src" .json)
store=$out/.local/share/opencode/storage
take opencode "$src" "$store/session/$(basename "$(dirname "$src")")/$ses.json" all
real=$(dirname "$(dirname "$(dirname "$src")")")
for msg in $(ls "$real/message/$ses" | sed -n 1,4p); do
  take opencode "$real/message/$ses/$msg" "$store/message/$ses/$msg" all
  for part in $(ls "$real/part/${msg%.json}" 2>/dev/null | sed -n 1,4p); do
    take opencode "$real/part/${msg%.json}/$part" "$store/part/${msg%.json}/$part" all
  done
done

mkdir -p "$repo"
echo "wrote $(find "$out" -type f | wc -l) files to $out"
grep -rlio "wassname\|/media/\|SGIronWolf" "$out" && echo "LEAK: the paths above still name a real place" || true
