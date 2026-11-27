#!/usr/bin/env bash
# Layout audit in the real webview (debug build with --features workspace-e2e). Synthetic data only.
# Prints one audit line per view and interface scale and saves screenshots next to the log.
set -euo pipefail
cd "$(dirname "$0")/.."
run=${1:-$(mktemp -d /tmp/agentview-overlap.XXXXXX)}
mkdir -p "$run";run=$(realpath "$run")
test ! -e "$run/data" || { echo 'Choose a fresh evidence directory';exit 2; }
mkdir -m700 "$run/data" "$run/config"
printf 'allow = []\n' > "$run/config/allowlist.toml";chmod 600 "$run/config/allowlist.toml"
# Synthetic Tasks card data: a tasks log, a transcript and a workflow record, so the populated card and table are audited too.
now=$(date +%s); sd="$run/data/signals"; mkdir -m700 "$sd"
tdir="$run/claude/projects/-srv-a-very-long-synthetic-project-directory-name/sess-long"; mkdir -p "$tdir/workflows"
tp="$run/claude/projects/-srv-a-very-long-synthetic-project-directory-name/long-session-0123456789abcdef.jsonl"
long='Refactor the incremental parser so that a very long first line of a prompt keeps wrapping inside its table cell without overlapping the next column or forcing a horizontal scroll'
iso() { date -u -d "@$1" +%Y-%m-%dT%H:%M:%S.250Z; }
printf '{"type":"assistant","timestamp":"%s","sessionId":"long-session-0123456789abcdef","message":{"id":"m1","usage":{"input_tokens":123456,"output_tokens":7890123,"cache_creation_input_tokens":456789,"cache_read_input_tokens":987654321}}}\n' "$(iso $((now-3000)))" > "$tp"
{ printf '{"t":%s,"s":"long-session-0123456789abcdef","c":"/srv/a-very-long-synthetic-project-directory-name","tp":"%s","k":"start","m":"%s"}\n' "$((now-3600))" "$tp" "$long"
  printf '{"t":%s,"s":"long-session-0123456789abcdef","c":"/srv/a-very-long-synthetic-project-directory-name","tp":"%s","k":"end"}\n' "$((now-2400))" "$tp"
  printf '{"t":%s,"s":"other-session","c":"/srv/short","k":"start","m":"Interrupted one"}\n' "$((now-2000))"
  printf '{"t":%s,"s":"other-session","c":"/srv/short","k":"start","m":"Still running"}\n' "$((now-60))"
  printf '{"t":%s,"s":"third-session","c":"/srv/short","k":"start","m":"No transcript path"}\n' "$((now-900))"
  printf '{"t":%s,"s":"third-session","c":"/srv/short","k":"end"}\n' "$((now-800))"; } > "$sd/claude-tasks.jsonl"
chmod 600 "$sd/claude-tasks.jsonl"
printf '{"runId":"wf_overlap-long-run-id-0123456789","agentCount":12,"durationMs":93784000,"workflowName":"a-very-long-workflow-name-that-must-not-overlap","status":"completed","startTime":%s000,"totalTokens":123456789012}' "$((now-5000))" > "$tdir/workflows/wf_overlap-long-run-id-0123456789.json"
printf 'not json' > "$tdir/workflows/wf_broken-1.json"
export AGENT_APP_TEST_CLAUDE_DIR="$run/claude"
export AGENT_APP_DATA_DIR="$run/data" AGENT_APP_CONFIG="$run/config/allowlist.toml" AGENT_APP_TEST_OVERLAP=1 AGENT_APP_TEST_PHASE=first
export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 GDK_BACKEND=x11
timeout --kill-after=8 120 xvfb-run -a -s '-screen 0 '"${AGENT_APP_TEST_SCREEN:-1440x1000}"'x24' bash -c '
  "$1" > "$2" 2>&1 & pid=$!
  trap '\''kill "$pid" 2>/dev/null || true'\'' EXIT
  for i in {1..1100};do
    for name in $(grep -o "capture=[a-z]*-[0-9]*" "$2" | cut -d= -f2 | sort -u);do
      [ -e "$3/$name.png" ] || import -window root "$3/$name.png"
    done
    kill -0 "$pid" 2>/dev/null || break
    sleep .1
  done
  wait "$pid"
' _ "$PWD/target/debug/agent-app" "$run/app.log" "$run" || { tail -35 "$run/app.log";exit 1; }
grep -E 'audit=|result=' "$run/app.log" | sed 's/%20/ /g;s/%7C/|/g' || true
