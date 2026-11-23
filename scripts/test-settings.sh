#!/usr/bin/env bash
# Real webview + private SQLite preferences; no native agent process or authentication.
set -euo pipefail
cd "$(dirname "$0")/.."
run=${1:-$(mktemp -d /tmp/agentview-settings.XXXXXX)}
mkdir -p "$run";run=$(realpath "$run")
test ! -e "$run/data" || { echo 'Choose a fresh settings evidence directory';exit 2; }
mkdir -m700 "$run/data" "$run/config"
printf 'allow = []\n' > "$run/config/allowlist.toml";chmod 600 "$run/config/allowlist.toml"
export AGENT_APP_DATA_DIR="$run/data" AGENT_APP_CONFIG="$run/config/allowlist.toml" AGENT_APP_TEST_SETTINGS=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 GDK_BACKEND=x11
for phase in first restart corrupt future;do
  if [ "$phase" = corrupt ] || [ "$phase" = future ];then
    python3 - "$run" "$phase" <<'PYINJECT'
import json,sqlite3,sys
from pathlib import Path
run=Path(sys.argv[1]);phase=sys.argv[2]
with sqlite3.connect(run/'data/workspace.sqlite3') as db:
    raw='{broken' if phase=='corrupt' else json.dumps({'version':2,'settings':{'future_private':'FUTURE_PRIVATE_MARKER'}})
    db.execute('UPDATE app_settings SET revision=revision+1,format_version=?,settings_json=? WHERE singleton=1',(1 if phase=='corrupt' else 2,raw))
    if phase=='future':assert db.execute('SELECT count(*) FROM app_settings_recovery').fetchone()[0]>=1
PYINJECT
  fi
  export AGENT_APP_TEST_PHASE="$phase"
  timeout --kill-after=8 65 xvfb-run -a -s '-screen 0 1440x1000x24' bash -c '
    "$1" > "$2" 2>&1 & pid=$!
    trap '\''kill "$pid" 2>/dev/null || true'\'' EXIT
    for i in {1..550};do
      for page in settings-light settings-dark;do
        if grep -q "WORKSPACE_E2E capture=$page" "$2" && [ ! -e "${3%.png}-$page.png" ];then import -window root "${3%.png}-$page.png";fi
      done
      if grep -q "WORKSPACE_E2E phase=" "$2" && [ ! -e "$3" ];then import -window root "$3";fi
      kill -0 "$pid" 2>/dev/null || break
      sleep .1
    done
    wait "$pid"
  ' _ "$PWD/target/debug/agent-app" "$run/$phase.log" "$run/$phase.png" || { tail -35 "$run/$phase.log";exit 1; }
  grep '^WORKSPACE_E2E ' "$run/$phase.log"
  grep -qx 'WORKSPACE_E2E result=pass' "$run/$phase.log"
done
python3 - "$run" <<'PYCHECK'
import json,os,sqlite3,stat,sys
from pathlib import Path
run=Path(sys.argv[1]);reports=list((run/'data/exports').glob('*.json'));assert reports
for path in reports:
    text=path.read_text();report=json.loads(text)
    assert 'DIAGNOSTIC_SECRET_MARKER' not in text and 'FUTURE_PRIVATE_MARKER' not in text
    assert str(run) not in text and '/home/' not in text
    assert stat.S_IMODE(path.stat().st_mode)==0o600
    assert report['report_format']==1
with sqlite3.connect(run/'data/workspace.sqlite3') as db:
    version,raw=db.execute('SELECT format_version,settings_json FROM app_settings').fetchone()
    assert version==2 and 'FUTURE_PRIVATE_MARKER' in raw
    assert db.execute('SELECT count(*) FROM agents').fetchone()[0]==14
    assert db.execute('SELECT count(*) FROM app_settings_recovery').fetchone()[0]>=1
with sqlite3.connect(run/'data/sessions/sessions.sqlite3') as db:assert db.execute('SELECT count(*) FROM sessions').fetchone()[0]==0
print('Settings persistence/recovery, diagnostic privacy and no agent launch: PASS')
PYCHECK
