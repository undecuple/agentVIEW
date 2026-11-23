#!/usr/bin/env bash
# Actual native close event; isolated synthetic provider/helper only.
set -euo pipefail
cd "$(dirname "$0")/.."
run=${1:-$(mktemp -d /tmp/agentview-live-cancel.XXXXXX)}
mkdir -p "$run";run=$(realpath "$run")
test ! -e "$run/data" || { echo 'Choose fresh evidence directory';exit 2; }
mkdir -m700 "$run/data" "$run/config" "$run/alpha" "$run/beta"
printf 'allow = []\n' > "$run/config/allowlist.toml";chmod 600 "$run/config/allowlist.toml"
for provider in claude codex; do
  printf '#!%s\n' "$(command -v python3)" > "$run/$provider-fixture"
  tail -n +2 tests/live-cli-fixture.py >> "$run/$provider-fixture";chmod 700 "$run/$provider-fixture"
done
touch "$run/beta/.hang-bootstrap"
export AGENT_APP_DATA_DIR="$run/data" AGENT_APP_CONFIG="$run/config/allowlist.toml"
export AGENT_APP_TEST_LIVE=1 AGENT_APP_TEST_CANCEL=1 AGENT_APP_TEST_PROJECT_ROOT="$run"
export AGENT_APP_TEST_CLAUDE="$run/claude-fixture" AGENT_APP_TEST_CODEX="$run/codex-fixture"
export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 GDK_BACKEND=x11
timeout --kill-after=5 20 xvfb-run -a target/debug/agent-app > "$run/app.log" 2>&1 || { tail -40 "$run/app.log";exit 1; }
python3 - "$run" <<'PY'
import os,sqlite3,sys,time
from pathlib import Path
run=Path(sys.argv[1]);log=(run/'app.log').read_text()
assert 'WORKSPACE_E2E phase=native-close-ready' in log
assert 'WORKSPACE_E2E close=deferred' in log and 'WORKSPACE_E2E close=accepted' in log
assert time.time()-(run/'beta/.helper-started').stat().st_mtime<6,'close stalled waiting for bootstrap timeout'
for path in [run/'alpha/.fixture-pid',run/'beta/.helper-started']:
    pid=int(path.read_text())
    try:os.kill(pid,0)
    except ProcessLookupError:pass
    else:raise AssertionError('Owned fixture process survived native close')
with sqlite3.connect(run/'data/sessions/sessions.sqlite3') as connection:
    records=connection.execute('SELECT provider,status,provider_session_id FROM sessions ORDER BY provider').fetchall()
assert records[0][0:2]==('claude','stopped'),records
assert records[1]==('codex','bootstrap_unknown',None),records
print('Native close cancels stalled bootstrap, stops other provider, and preserves uncertain identity: PASS')
PY
