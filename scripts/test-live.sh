#!/usr/bin/env bash
# Two-process GUI lifecycle test with synthetic CLI; no account/network/inference.
set -euo pipefail
cd "$(dirname "$0")/.."
run=${1:-$(mktemp -d /tmp/agentview-live.XXXXXX)}
mkdir -p "$run"; run=$(realpath "$run")
test ! -e "$run/data" || { echo 'Choose fresh evidence directory'; exit 2; }
mkdir -m700 "$run/data" "$run/config" "$run/alpha" "$run/beta"
printf 'allow = []\n' > "$run/config/allowlist.toml"
chmod 600 "$run/config/allowlist.toml"
# Fixed interpreter path from the pinned shell; no environment-selected CLI in release.
for provider in claude codex; do
  printf '#!%s\n' "$(command -v python3)" > "$run/$provider-fixture"
  tail -n +2 tests/live-cli-fixture.py >> "$run/$provider-fixture"; chmod 700 "$run/$provider-fixture"
done
export AGENT_APP_DATA_DIR="$run/data" AGENT_APP_CONFIG="$run/config/allowlist.toml"
export AGENT_APP_TEST_POLL_MS=300 AGENT_APP_TEST_LIVE=1 AGENT_APP_TEST_CLAUDE="$run/claude-fixture" AGENT_APP_TEST_CODEX="$run/codex-fixture" AGENT_APP_TEST_PROJECT_ROOT="$run"
export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 GDK_BACKEND=x11
for phase in first restart; do
  export AGENT_APP_TEST_PHASE="$phase"
  timeout --kill-after=10 60 xvfb-run -a -s '-screen 0 1440x1000x24' bash -c '
    "$1" > "$2" 2>&1 & pid=$!
    trap '\''kill "$pid" 2>/dev/null || true'\'' EXIT
    for i in {1..400}; do
      for page in usage usage-daily; do
        if grep -q "WORKSPACE_E2E capture=$page" "$2" && [ ! -e "${3%.png}-$page.png" ]; then import -window root "${3%.png}-$page.png"; fi
      done
      if grep -q "WORKSPACE_E2E phase=" "$2"; then import -window root "$3"; break; fi
      kill -0 "$pid" 2>/dev/null || break
      sleep .1
    done
    wait "$pid"
  ' _ "$PWD/target/debug/agent-app" "$run/$phase.log" "$run/$phase.png" || { tail -45 "$run/$phase.log"; exit 1; }
  grep '^WORKSPACE_E2E ' "$run/$phase.log"
  grep -qx 'WORKSPACE_E2E result=pass' "$run/$phase.log"
done
python3 - "$run" <<'PY'
import json,sys
from pathlib import Path
run=Path(sys.argv[1])
a=json.loads((run/'alpha/.agentview-fixture.json').read_text())
b=json.loads((run/'beta/.agentview-fixture.json').read_text())
assert a['id']!=b['id'] and len(a['turns'])==len(b['turns'])==1
assert a['provider']=='claude' and b['provider']=='codex'
assert (run/'beta/.bootstrap-count').read_text()=='1'
assert b'SYNTHETIC_CONTENT_MUST_NOT_PERSIST' not in (run/'data/sessions/sessions.sqlite3').read_bytes()
print('GUI fixture isolation and no input replay: PASS')
PY
echo "Live GUI checks: PASS ($run)"
