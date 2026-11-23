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
