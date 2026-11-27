#!/usr/bin/env bash
# Claude Notification-hook signal -> webhook message, against a fake curl. Nothing leaves this machine.
set -euo pipefail
cd "$(dirname "$0")/.."
run=${1:-$(mktemp -d /tmp/agentview-hooksig.XXXXXX)}
mkdir -p "$run";run=$(realpath "$run")
test ! -e "$run/data" || { echo 'Choose a fresh evidence directory';exit 2; }
mkdir -m700 "$run/data" "$run/config"
printf 'allow = []\n' > "$run/config/allowlist.toml";chmod 600 "$run/config/allowlist.toml"
cat > "$run/fake-curl" <<SH
#!/usr/bin/env bash
{ echo "ARGS: \$*"; echo "STDIN: \$(cat)"; } >> "$run/curl.log"
printf 204
SH
chmod 700 "$run/fake-curl"
cat > "$run/fake-notify-send" <<SH
#!/usr/bin/env bash
echo "NS: \$*" >> "$run/notify.log"
SH
chmod 700 "$run/fake-notify-send"
touch "$run/data/board-alert"; chmod 600 "$run/data/board-alert"
export AGENT_APP_DATA_DIR="$run/data" AGENT_APP_CONFIG="$run/config/allowlist.toml" AGENT_APP_TEST_CURL="$run/fake-curl" AGENT_APP_TEST_NOTIFYSEND="$run/fake-notify-send" AGENT_APP_TEST_HOOKSIG=1 AGENT_APP_TEST_POLL_MS=300 AGENT_APP_TEST_PHASE=first
mkdir -p "$run/claude/projects/-srv-example-project/sess-1/workflows"
printf '{"runId":"wf_fixture-1","script":"SECRET-SCRIPT","result":["SECRET-OUTPUT"],"agentCount":3,"logs":["SECRET-LOG"],"durationMs":5000,"summary":"SECRET-SUMMARY","workflowName":"fixture","status":"completed","startTime":%s000,"totalTokens":1234}' "$(( $(date +%s) - 60 ))" > "$run/claude/projects/-srv-example-project/sess-1/workflows/wf_fixture-1.json"
printf 'not json' > "$run/claude/projects/-srv-example-project/sess-1/workflows/wf_broken-1.json"
export AGENT_APP_TEST_CLAUDE_DIR="$run/claude"
export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 GDK_BACKEND=x11
sig="$run/data/signals/claude-notify.jsonl"
# Appends lines once the page reports ready: one old line is ignored by design (written before the app reads), then real ones.
( for i in $(seq 1 600); do grep -q 'ready=1' "$run/hook.log" 2>/dev/null && break; sleep 0.1; done
  printf '{"t":%s,"s":"sess-1","c":"/work/example-project","k":"permission_prompt","m":"Claude needs your permission to use Bash"}\n' "$(date +%s)" >> "$sig"
  printf '{"t":%s,"s":"sess-1","c":"/work/example-project","k":"permission_prompt","m":"Claude needs your permission to use Bash"}\n' "$(date +%s)" >> "$sig"
  printf '{"t":%s,"s":"sess-2","c":"/work/other","k":"auth_success","m":"signed in"}\n' "$(date +%s)" >> "$sig"
  printf '{"t":%s,"s":"019a-codex-1","c":"/work/codex-proj","k":"turn_complete","m":""}\n' "$(date +%s)" >> "$run/data/signals/codex-notify.jsonl"
  printf '{"t":%s,"five":{"used_percentage":40,"resets_at":%s},"seven":{"used_percentage":85,"resets_at":%s}}' "$(date +%s)" "$(( $(date +%s) + 10000 ))" "$(( $(date +%s) + 200000 ))" > "$run/data/signals/claude-limits.json"
  mkdir -p "$run/data/mailbox"; chmod 700 "$run/data/mailbox"; mb="$run/data/mailbox/messages.jsonl"
  printf '{"t":%s,"f":"worker-1","to":"coord","m":"tests pass, ready for review"}\n' "$(date +%s)" >> "$mb"
  printf '{"t":%s,"f":"bad name","to":"coord","m":"ignored"}\n' "$(date +%s)" >> "$mb"
  ff="$run/data/signals/claude-files.jsonl"
  printf '{"t":%s,"s":"sess-1","c":"/srv/example-project","k":"Read","p":"/srv/example-project/src/lib.rs"}\n' "$(( $(date +%s) - 3 ))" >> "$ff"
  printf '{"t":%s,"s":"sess-1","c":"/srv/example-project","k":"Edit","p":"/srv/example-project/README.md"}\n' "$(( $(date +%s) - 2 ))" >> "$ff"
  printf '{"t":%s,"s":"sess-1","c":"/srv/example-project","k":"Read","p":"/srv/someone/.ssh/id_rsa"}\n' "$(( $(date +%s) - 1 ))" >> "$ff"
  printf '{"t":%s,"s":"sess-1","c":"/srv/example-project","k":"Bash","p":"/srv/x"}\n' "$(date +%s)" >> "$ff"
  tf="$run/data/signals/claude-tasks.jsonl"
  # The hook commands the app generated (reported by the page), run with jq on sample Claude hook payloads: two sessions interleaved,
  # a transcript with lines before, inside and after the task window, another session inside it, a missing end, a key in the prompt,
  # a payload without transcript_path.
  cmd_of() { grep -o "taskscmd-$1=[^ ]*" "$run/hook.log" | head -1 | cut -d= -f2- | python3 -c 'import sys,urllib.parse;print(urllib.parse.unquote(sys.stdin.read().strip()),end="")'; }
  startcmd=$(cmd_of start); endcmd=$(cmd_of end)
  tdir="$run/claude/projects/-srv-example-project"
  key="sk-ant-$(printf 'a1b2c3d4e5%.0s' 1 2 3 4)"
  pl() { jq -nc --arg s "$1" --arg e "$2" --arg p "$3" --arg tp "$4" '{session_id:$s,cwd:"/srv/example-project",hook_event_name:$e} + (if $e=="UserPromptSubmit" then {prompt:$p} else {} end) + (if $tp=="" then {} else {transcript_path:$tp} end)'; }
  tl() { printf '{"type":"assistant","timestamp":"%s","sessionId":"%s","message":{"id":"%s","content":[{"type":"text","text":"CONTENT-MARKER"}],"usage":{"input_tokens":%s,"output_tokens":%s,"cache_creation_input_tokens":%s,"cache_read_input_tokens":%s}}}\n' "$(date -u -d "$2" +%Y-%m-%dT%H:%M:%S.500Z)" "$3" "$4" "$5" "$6" "$7" "$8" >> "$1"; }
  ta="$tdir/hk-a.jsonl"; tb="$tdir/hk-b.jsonl"
  tl "$ta" "-100 sec" hk-a before 1 1 1 1
  pl hk-a UserPromptSubmit $'Tokens please\nsecond line SECOND-LINE' "$ta" | sh -c "$startcmd"
  pl hk-b UserPromptSubmit "Other session task" "$tb" | sh -c "$startcmd"
  pl hk-c UserPromptSubmit "Interrupted one" "" | sh -c "$startcmd"
  sleep 1
  pl hk-c UserPromptSubmit "Running one with $key inside" "" | sh -c "$startcmd"
  pl hk-d UserPromptSubmit "No transcript path" "" | sh -c "$startcmd"
  sleep 1
  tl "$ta" "now" hk-a m1 10 20 30 400
  tl "$ta" "now" hk-other other 999 999 999 999
  tl "$tb" "now" hk-b m2 1 2 3 4
  sleep 1
  pl hk-b Stop "" "$tb" | sh -c "$endcmd"
  pl hk-a Stop "" "$ta" | sh -c "$endcmd"
  pl hk-d Stop "" "" | sh -c "$endcmd"
  sleep 2
  tl "$ta" "now" hk-a after 7 7 7 7
  printf '{"t":%s,"s":"sess-1","c":"/srv/example-project","k":"start","m":"Fix the parser"}\n' "$(( $(date +%s) - 30 ))" >> "$tf"
  printf '{"t":%s,"s":"sess-1","c":"/srv/example-project","k":"end"}\n' "$(( $(date +%s) - 20 ))" >> "$tf"
  printf '{"t":%s,"s":"sess-1","c":"/srv/example-project","k":"start","m":"Second task"}\n' "$(( $(date +%s) - 10 ))" >> "$tf"
  printf '{"t":%s,"s":"sess-1","c":"/srv/example-project","k":"pause"}\n' "$(date +%s)" >> "$tf"
  mkdir -p "$run/data/kanban"; chmod 700 "$run/data/kanban"; kb="$run/data/kanban/proj-a.jsonl"; n=$(date +%s)
  printf '{"t":%s,"id":"T-1","col":"queued","title":"Write tests","by":"worker-1"}\n' "$((n-30))" >> "$kb"
  printf '{"t":%s,"id":"T-1","col":"active","note":"half done"}\n' "$((n-10))" >> "$kb"
  printf '{"t":%s,"id":"T-2","col":"blocked","title":"Deploy","by":"worker-2","note":"needs review"}\n' "$n" >> "$kb"
  printf '{"t":%s,"id":"T-3","col":"active","title":"Old active","by":"worker-3"}\n' "$((n-7200))" >> "$kb"
  printf '{"t":%s,"id":"bad id","col":"active","title":"ignored"}\n' "$n" >> "$kb"
  mkdir -p "$run/data/queues"; chmod 700 "$run/data/queues"; qf="$run/data/queues/gpu.jsonl"; n=$(date +%s)
  printf '{"t":%s,"id":"run-1","ev":"running","who":"proj-a","est_min":10,"slot":"0"}\n' "$((n-1200))" >> "$qf"
  printf '{"t":%s,"id":"w-2","ev":"queued","who":"proj-c","est_min":5,"pos":2,"since":%s}\n' "$n" "$((n-60))" >> "$qf"
  printf '{"t":%s,"id":"w-1","ev":"queued","who":"proj-b","est_min":40,"pos":1,"since":%s,"note":"long probe"}\n' "$n" "$((n-300))" >> "$qf"
  printf '{"t":%s,"id":"res","ev":"reserved","who":"owner-x","note":"quiet run","until":%s}\n' "$n" "$((n+600))" >> "$qf"
  mkdir -p "$run/data/decisions"; chmod 700 "$run/data/decisions"; df="$run/data/decisions/pending.jsonl"; n=$(date +%s)
  printf '{"t":%s,"id":"Q-1","ev":"ask","f":"worker-1","p":"proj-a","q":"Use library A or B?","o":["A","B"]}\n' "$((n-20))" >> "$df"
  printf '{"t":%s,"id":"Q-2","ev":"ask","f":"worker-2","q":"Push now?"}\n' "$((n-10))" >> "$df"
  printf '{"t":%s,"id":"Q-2","ev":"answered"}\n' "$n" >> "$df"
  printf 'not json at all\n' >> "$sig" ) &
timeout --kill-after=8 90 xvfb-run -a -s '-screen 0 1440x1000x24' "$PWD/target/debug/agent-app" > "$run/hook.log" 2>&1 || { tail -30 "$run/hook.log";exit 1; }
wait
grep -E 'E2E (check|error|result)' "$run/hook.log" | sed 's/%20/ /g'
grep -qx 'WORKSPACE_E2E result=pass' "$run/hook.log"
n=$(grep -c 'waiting for your decision' "$run/curl.log" || true)
[ "$n" = 1 ] || { echo "expected exactly one waiting message (duplicate and non-waiting lines ignored), got $n"; cat "$run/curl.log"; exit 1; }
grep -q 'example-project' "$run/curl.log" || { echo 'message does not name the agent'; exit 1; }
if grep -q '/work/example-project' "$run/curl.log"; then echo 'folder path leaked into the message'; exit 1; fi
if grep '^ARGS:' "$run/curl.log" | grep -q 'SECRET-TOKEN'; then echo 'address leaked into arguments'; exit 1; fi
echo "Hook signal: PASS ($run)"

nd=$(grep -c 'waiting for your decision' "$run/notify.log" || true)
[ "$nd" = 1 ] || { echo "expected exactly one desktop notification for the Claude signal, got $nd"; cat "$run/notify.log"; exit 1; }
grep -q 'Codex) finished its turn' "$run/notify.log" || { echo "missing the Codex desktop notification"; cat "$run/notify.log"; exit 1; }
echo "hook signal + desktop notify checks ok"
grep -q 'plan limit, 7-day window at 85%' "$run/notify.log" || { echo "missing the plan-limit desktop notification"; cat "$run/notify.log"; exit 1; }
[ "$(grep -c 'plan limit, 7-day' "$run/notify.log")" = 1 ] || { echo "plan-limit desktop notification not once"; exit 1; }
grep -q 'plan limit, 7-day window at 85%' "$run/curl.log" || { echo "plan-limit webhook missing"; exit 1; }
echo "plan limit checks ok"
# Away mode: the flag and the instruction are private files written by the app; the decisions file was only read
[ -f "$run/data/away" ] && [ "$(stat -c %a "$run/data/away")" = 600 ] || { echo "away flag missing or wrong mode"; exit 1; }
grep -q 'AWAY MODE is ON' "$run/data/away-instruction.txt" && [ "$(stat -c %a "$run/data/away-instruction.txt")" = 600 ] || { echo "instruction file wrong"; exit 1; }
[ "$(grep -c . "$run/data/decisions/pending.jsonl")" = 3 ] || { echo "the app must not change the agents' decisions file"; exit 1; }
echo "Away mode files: PASS"
# The log is written only by the hooks: the generated commands put tp on their lines, and the app left the raw lines alone
[ "$(grep -c '"tp":"' "$run/data/signals/claude-tasks.jsonl")" -ge 4 ] || { echo "hook lines without tp"; exit 1; }
grep -q 'second line SECOND-LINE\|SECOND-LINE' "$run/data/signals/claude-tasks.jsonl" && { echo "the hook kept more than the first prompt line"; exit 1; }
echo "Tasks hook command checks: PASS"
