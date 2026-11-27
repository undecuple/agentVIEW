#!/usr/bin/env bash
# GUI test (private Xvfb, synthetic provider CLI, no account/network) for "run sessions in the background":
#   A default close stops the session and quits
#   B background close hides the window and keeps the session; a second launch brings it back (a stale socket from a
#     "crashed" instance is in the way first); `agentview --quit` stops the session and exits
#   C a hidden app exits by itself when its last session ends; `--quit` with nothing running is harmless
#   D SIGTERM to a hidden app stops the session, removes the socket and exits
#   E a first instance that owns the socket but never answers does not block a new start (default mode)
#   F the same, but holding the sessions store: the new start refuses with a message and exit status 1
# Window visibility is asserted with xwininfo (Map State) on the private Xvfb, not only from log lines.
# Needs target/debug/agent-app built with --features workspace-e2e.
set -euo pipefail
cd "$(dirname "$0")/.."
root=${1:-$(mktemp -d /tmp/agentview-bg.XXXXXX)}
mkdir -p "$root"; root=$(realpath "$root")
bin=$PWD/target/debug/agent-app
pids=()
trap 'for p in "${pids[@]:-}"; do [ -n "$p" ] && kill "$p" 2>/dev/null || true; done' EXIT
fail() { echo "FAIL: $*"; for l in "$root"/*/app.log; do echo "--- $l"; tail -25 "$l"; done; exit 1; }

setup() { # $1 = scenario name
  local run="$root/$1"
  test ! -e "$run" || { echo "Choose a fresh evidence directory"; exit 2; }
  mkdir -m700 "$run" "$run/data" "$run/config" "$run/alpha"
  printf 'allow = []\n' > "$run/config/allowlist.toml"; chmod 600 "$run/config/allowlist.toml"
  for provider in claude codex; do
    printf '#!%s\n' "$(command -v python3)" > "$run/$provider-fixture"
    tail -n +2 tests/live-cli-fixture.py >> "$run/$provider-fixture"; chmod 700 "$run/$provider-fixture"
  done
}
envs() { # $1 = scenario name, $2 = mode
  local run="$root/$1"
  export AGENT_APP_DATA_DIR="$run/data" AGENT_APP_CONFIG="$run/config/allowlist.toml"
  export AGENT_APP_TEST_POLL_MS=300 AGENT_APP_TEST_LIVE=1 AGENT_APP_TEST_BG="$2" AGENT_APP_TEST_PROJECT_ROOT="$run"
  export AGENT_APP_TEST_CLAUDE="$run/claude-fixture" AGENT_APP_TEST_CODEX="$run/codex-fixture"
  export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 GDK_BACKEND=x11
}
launch() { # starts the app in a private Xvfb in the background; sets $app
  timeout --kill-after=5 120 xvfb-run -a -s '-screen 0 1440x1000x24' sh -c 'echo "$DISPLAY" > "$1"; exec "$2"' sh "$root/$1/display" "$bin" > "$root/$1/app.log" 2>&1 &
  app=$!; pids+=("$app")
}
waitlog() { # $1 scenario, $2 pattern, $3 seconds
  for _ in $(seq 1 $(($3 * 10))); do
    grep -q "$2" "$root/$1/app.log" && return 0
    kill -0 "$app" 2>/dev/null || { grep -q "$2" "$root/$1/app.log" && return 0; return 1; }
    sleep .1
  done; return 1
}
alive() { kill -0 "$1" 2>/dev/null; }
command -v xwininfo >/dev/null || { echo "xwininfo is missing: run inside nix develop (the flake dev shell provides it)"; exit 2; }
visible() { # $1 scenario: number of mapped (viewable) top-level windows titled like the workspace window
  local d id n=0; d=$(cat "$root/$1/display" 2>/dev/null) || { echo 0; return; }
  for id in $(xwininfo -display "$d" -root -tree 2>/dev/null | grep -F 'Agent workspace' | grep -o '0x[0-9a-f]*' | sort -u); do
    xwininfo -display "$d" -id "$id" 2>/dev/null | grep -q 'Map State: IsViewable' && n=$((n + 1))
  done; echo "$n"
}
wait_visible() { # $1 scenario, $2 expected (0 = none, 1 = at least one), $3 seconds
  local n
  for _ in $(seq 1 $(($3 * 10))); do
    n=$(visible "$1"); { [ "$2" = 0 ] && [ "$n" = 0 ]; } && return 0; { [ "$2" != 0 ] && [ "$n" -ge 1 ]; } && return 0; sleep .1
  done; return 1
}
hung_instance() { # $1 scenario, $2 = hold: also hold the sessions store lock. A "first instance" that never answers.
  python3 -I - "$root/$1/data" "$2" <<'PY' &
import fcntl,os,socket,sys,time
d,hold=sys.argv[1],sys.argv[2]=='hold'
if hold:
    os.makedirs(d+'/sessions',mode=0o700,exist_ok=True)
    f=open(d+'/sessions/manager.lock','w'); fcntl.flock(f,fcntl.LOCK_EX|fcntl.LOCK_NB)
s=socket.socket(socket.AF_UNIX); s.bind(d+'/control.sock'); os.chmod(d+'/control.sock',0o600); s.listen(128)
time.sleep(90)
PY
  pids+=("$!"); hung=$!
  for _ in $(seq 1 50); do test -S "$root/$1/data/control.sock" && return 0; sleep .1; done; return 1
}
nodisplay() { env -u DISPLAY -u WAYLAND_DISPLAY "$@"; }

# ---- A: default close stops and quits ------------------------------------------------------------------------
setup a; envs a default; launch a
waitlog a 'WORKSPACE_E2E phase=session-ready' 60 || fail "A: session never became ready"
a_pid=$(cat "$root/a/alpha/.fixture-pid")
wait "$app" && rc=0 || rc=$?
grep -q 'WORKSPACE_E2E close=accepted' "$root/a/app.log" || fail "A: close was not accepted"
! grep -q 'close=hidden' "$root/a/app.log" || fail "A: default close must not hide"
! alive "$a_pid" || fail "A: session survived the default close"
test ! -e "$root/a/data/control.sock" || fail "A: control socket left behind after a clean exit"
echo "A default close stops the session and quits: PASS (exit $rc)"

# ---- B: background close, show, quit -----------------------------------------------------------------------------
setup b; envs b background
python3 -I - "$root/b/data/control.sock" <<'PY'
import socket,sys
s=socket.socket(socket.AF_UNIX);s.bind(sys.argv[1]);s.close()  # a socket file nobody listens on: a crashed instance
PY
test -S "$root/b/data/control.sock" || fail "B: could not create the stale socket"
launch b
waitlog b 'WORKSPACE_E2E phase=session-ready' 60 || fail "B: session never became ready"
wait_visible b 1 10 || fail "B: the window is not mapped before the close"
waitlog b 'WORKSPACE_E2E close=hidden' 60 || fail "B: window was not hidden"
wait_visible b 0 10 || fail "B: the window is still mapped after hide (hide() did nothing)"
echo "  window mapped before close, unmapped after hide"
b_pid=$(cat "$root/b/alpha/.fixture-pid")
sleep 3 # past two idle-watcher ticks: a running session keeps the hidden app alive
alive "$app" || fail "B: app exited although a session runs"
alive "$b_pid" || fail "B: session died when the window was hidden"
[ "$(stat -c %a "$root/b/data/control.sock")" = 600 ] || fail "B: socket is not mode 600"
out=$(nodisplay timeout 10 "$bin") || fail "B: second launch failed"
echo "second launch said: $out"
echo "$out" | grep -q 'already running' || fail "B: second launch did not hand over"
waitlog b 'WORKSPACE_E2E phase=after-show' 60 || fail "B: page did not report after show"
wait_visible b 1 10 || fail "B: the window is not mapped again after show"
echo "  window mapped again after show"
! grep -q 'WORKSPACE_E2E error' "$root/b/app.log" || fail "B: page reported an error"
grep -q 'WORKSPACE_E2E check=terminal%20buffer%20kept' "$root/b/app.log" || fail "B: buffer check missing"
alive "$b_pid" || fail "B: session died after show"
out=$(nodisplay timeout 10 "$bin" --quit) || fail "B: --quit failed"
echo "--quit said: $out"
wait "$app" && rc=0 || rc=$?
grep -q 'WORKSPACE_E2E control=quit why=control stopped=true' "$root/b/app.log" || fail "B: quit not logged"
! alive "$b_pid" || fail "B: session survived --quit"
test ! -e "$root/b/data/control.sock" || fail "B: socket left behind after --quit"
python3 -I - "$root/b/data/sessions/sessions.sqlite3" <<'PY'
import sqlite3,sys
rows=sqlite3.connect(sys.argv[1]).execute("SELECT status FROM sessions").fetchall()
assert rows==[('stopped',)],rows
PY
echo "B background close hides, keeps the session, show restores it, --quit stops it: PASS (exit $rc)"
grep -E 'WORKSPACE_E2E (check|terminal_dom)' "$root/b/app.log" | sed 's/^/  /'

# ---- C: hidden app exits when the last session ends ------------------------------------------------------------
setup c; envs c background; launch c
waitlog c 'WORKSPACE_E2E close=hidden' 60 || fail "C: window was not hidden"
c_pid=$(cat "$root/c/alpha/.fixture-pid")
alive "$c_pid" && kill "$c_pid"
waitlog c 'WORKSPACE_E2E control=quit why=idle' 30 || fail "C: hidden app did not exit after its last session ended"
wait "$app" && rc=0 || rc=$?
test ! -e "$root/c/data/control.sock" || fail "C: socket left behind"
out=$(nodisplay timeout 10 "$bin" --quit) || fail "C: --quit with nothing running failed"
echo "$out" | grep -q 'not running' || fail "C: unexpected --quit output: $out"
echo "C hidden app exits by itself when no session runs; --quit with nothing running is harmless: PASS (exit $rc)"

# ---- D: a terminating signal to a hidden app takes the quit path --------------------------------------------------------
setup d; envs d background; launch d
waitlog d 'WORKSPACE_E2E close=hidden' 60 || fail "D: window was not hidden"
d_pid=$(cat "$root/d/alpha/.fixture-pid")
app_pid=$(sed -n 's/^WORKSPACE_E2E pid=\([0-9]*\)$/\1/p' "$root/d/app.log" | head -1)
[ -n "$app_pid" ] || fail "D: the app did not report its pid"
kill -TERM "$app_pid"
waitlog d 'WORKSPACE_E2E control=quit why=signal stopped=true' 30 || fail "D: SIGTERM did not take the quit path"
wait "$app" && rc=0 || rc=$?
! alive "$d_pid" || fail "D: session survived SIGTERM"
test ! -e "$root/d/data/control.sock" || fail "D: socket left behind after SIGTERM"
echo "D SIGTERM to a hidden app stops the session, removes the socket and exits: PASS (exit $rc)"

# ---- E: an unresponsive first instance does not block a new start -------------------------------------------------
setup e; envs e default; hung_instance e free || fail "E: could not start the unresponsive instance"
launch e
waitlog e 'WORKSPACE_E2E phase=session-ready' 90 || fail "E: the new app did not start next to an unresponsive instance"
grep -q 'does not answer (.*); starting without background control' "$root/e/app.log" || fail "E: no explanation on stderr"
wait "$app" && rc=0 || rc=$?
kill "$hung" 2>/dev/null || true
echo "E unresponsive first instance (store not held): the new app starts normally: PASS (exit $rc)"

# ---- F: unresponsive first instance holding the sessions store: refuse with a message ---------------------------------
setup f; envs f default; hung_instance f hold || fail "F: could not start the unresponsive instance"
launch f
wait "$app" && rc=0 || rc=$?
kill "$hung" 2>/dev/null || true
[ "$rc" = 1 ] || fail "F: expected exit status 1, got $rc"
grep -q 'holds the sessions store' "$root/f/app.log" || fail "F: no clear message"
! grep -q 'WORKSPACE_E2E phase=' "$root/f/app.log" || fail "F: the app started anyway"
echo "F unresponsive first instance holding the sessions store: refused with a message, exit 1: PASS"
echo "background GUI checks: PASS ($root)"
