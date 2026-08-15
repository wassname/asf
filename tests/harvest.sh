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
               "parentId","parentID","messageId","messageID","promptId","callID",
               "model","provider","version","timestamp","status","source","namespace",
               "finish_reason","state","kind","projectHash"];
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
    fix'
}

take() { # $1 agent, $2 source file, $3 dest, $4 lines, or "all" for one whole json
  mkdir -p "$(dirname "$3")"
  if [ "$4" = all ]; then cat "$2"; else head -n "$4" "$2"; fi | scrub "$1" >"$3"
}

rm -r "$out" 2>/dev/null || true
mkdir -p "$out"

# claude: the name records live at the end of the file, so take the last of each kind too.
# The earlier copies are relabelled stale, because reading the first one instead of the last
# is the bug this fixture is here to catch.
src=$($asf --paths -a claude -n 1)
dst=$out/.claude/projects/-tmp-asf-fixture-repo/$(basename "$src")
take claude "$src" "$dst" 30
sed -i 's/fixture claude [a-zA-Z]*"/fixture claude stale"/g' "$dst"
for kind in ai-title custom-title agent-name; do
  # this session never got an ai-title, so borrow the record from one that did
  with=$src
  grep -q "\"type\":\"$kind\"" "$src" || with=$(grep -rl "\"type\":\"$kind\"" ~/.claude/projects --include=*.jsonl | sed -n 1p)
  grep "\"type\":\"$kind\"" "$with" | tail -1 | scrub claude >>"$dst"
done
# and one of its subagent logs, which must stay hidden: 703 of 869 files here are these
sub=$(find ~/.claude/projects -path '*/subagents/*.jsonl' -printf '%T@ %p\n' | sort -rn | sed -n 2p | cut -d' ' -f2)
take claude "$sub" "${dst%.jsonl}/subagents/$(basename "$sub")" 12

# codex: one real session, and one subagent run that must stay hidden
src=$($asf --paths -a codex -n 1)
take codex "$src" "$out/.codex/sessions/2026/08/15/$(basename "$src")" 30
src=$($asf --sub --paths -a codex -n 40 | xargs grep -l '"subagent"' | sed -n 1p)
take codex "$src" "$out/.codex/sessions/2026/08/15/$(basename "$src")" 12

# pi: one session it minted a uuid for, and one a tool named, which must stay hidden. The
# name a tool chose is the person's words, so it is renamed in the file and in the id.
src=$($asf --paths -a pi -n 1)
take pi "$src" "$out/.pi/agent/sessions/--tmp-asf-fixture-repo--/$(basename "$src")" 30
src=$($asf --sub --paths -a pi -n 200 | grep -E '_(rev|oracle|panel)-' | sed -n 1p)
name=$(basename "$src"); id=${name#*_}; id=${id%.jsonl}
dst=$out/.pi/agent/sessions/--tmp-asf-fixture-repo--/${name%%_*}_rev-fixture-1.jsonl
take pi "$src" "$dst" 12
sed -i "s/$id/rev-fixture-1/g" "$dst"

# copilot
src=$($asf --paths -a copilot -n 1)
take copilot "$src" "$out/.copilot/session-state/$(basename "$(dirname "$src")")/events.jsonl" 30

# gemini: logs.json is one array for the whole project, not jsonl
src=$($asf --paths -a gemini -n 1)
dst=$out/.gemini/tmp/fixture-project/logs.json
mkdir -p "$(dirname "$dst")"
jq -c '.[0:6]' "$src" | scrub gemini >"$dst"

# opencode: a session is its json plus a directory of messages and one of parts each
src=$($asf --paths -a opencode -n 1)
ses=$(basename "$src" .json)
store=$out/.local/share/opencode/storage
# the directory opencode keeps it under is a hash of the real project path, so rename it
take opencode "$src" "$store/session/fixtureproject/$ses.json" all
real=$(dirname "$(dirname "$(dirname "$src")")")
for msg in $(ls "$real/message/$ses" | sed -n 1,4p); do
  take opencode "$real/message/$ses/$msg" "$store/message/$ses/$msg" all
  for part in $(ls "$real/part/${msg%.json}" 2>/dev/null | sed -n 1,4p); do
    take opencode "$real/part/${msg%.json}/$part" "$store/part/${msg%.json}/$part" all
  done
done

# hermes: a sqlite db, not files. Keep the real schema, replace the text, and make the second
# session a child so the subagent rule has something to hide.
real=$HOME/.hermes/state.db
db=$out/.hermes/state.db
mkdir -p "$(dirname "$db")"
sqlite3 "file:$real?mode=ro" \
  "select sql || ';' from sqlite_master where type='table' and name in ('sessions','messages')" |
  sqlite3 "$db"
sqlite3 "$db" <<SQL
attach 'file:$real?mode=ro' as real;
insert into sessions select * from real.sessions
  where cwd is not null and message_count > 0 order by started_at desc limit 3;
insert into messages select id, session_id, role, content, tool_call_id, tool_calls, tool_name,
  effect_disposition, timestamp, token_count, finish_reason, reasoning, reasoning_content,
  reasoning_details, codex_reasoning_items, codex_message_items, platform_message_id, observed,
  active, compacted, api_content, display_kind, display_metadata from (
  select m.*, row_number() over (partition by m.session_id order by m.timestamp) as seq
  from real.messages m where m.session_id in (select id from sessions)) where seq <= 6;
detach real;
update sessions set title = 'fixture hermes ' || rowid, display_name = null, cwd = '/tmp/asf-fixture-repo',
  git_repo_root = '/tmp/asf-fixture-repo', system_prompt = null, origin_json = null, git_branch = null,
  session_key = null, chat_id = null, user_id = null, model_config = null, handoff_state = null;
update sessions set parent_session_id = (select min(id) from sessions)
  where id = (select max(id) from sessions);
update messages set content = 'the hermes widget factory ships on tuesday', api_content = null,
  reasoning = null, reasoning_content = null, reasoning_details = null, tool_calls = null,
  codex_reasoning_items = null, codex_message_items = null, display_metadata = null;
update messages set tool_calls = '[{"name":"bash","arguments":{}}]', tool_name = 'bash'
  where id = (select min(id) from messages where role = 'assistant');
update messages set reasoning = 'the hermes widget factory counts its boxes'
  where id = (select max(id) from messages where role = 'assistant');
vacuum;
SQL

mkdir -p "$repo"
echo "wrote $(find "$out" -type f | wc -l) files to $out"
# names leak as easily as contents: a project directory and a session id are both someone's words
{ grep -rlio "$USER\|/media/\|/home/\|SGIronWolf" "$out"; find "$out" | grep -i "$USER\|SGIronWolf"; } &&
  echo "LEAK: the paths above still name a real place" || true
