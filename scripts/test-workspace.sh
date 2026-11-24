#!/usr/bin/env bash
# Runs the actual debug workspace twice against one task-owned database. No real desktop/session access.
# Run inside nix develop; build first with --features workspace-e2e.
set -euo pipefail
cd "$(dirname "$0")/.."
bin="$PWD/target/debug/agent-app"
run_dir=${1:-"$(mktemp -d /tmp/agent-app-workspace.XXXXXX)"}
mkdir -p "$run_dir/data" "$run_dir/config"; chmod 700 "$run_dir/data" "$run_dir/config"
test ! -e "$run_dir/data/workspace.sqlite3" || { echo "Choose a fresh test directory; existing data preserved" >&2; exit 2; }
run_dir=$(realpath "$run_dir")
printf 'allow = ["%s"]\n' "$run_dir/config" > "$run_dir/config/allowlist.toml"
chmod 600 "$run_dir/config/allowlist.toml"
for phase in first restart; do
  log="$run_dir/$phase.log"
  export AGENT_APP_DATA_DIR="$run_dir/data" AGENT_APP_CONFIG="$run_dir/config/allowlist.toml" AGENT_APP_TEST_PHASE="$phase"
  export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 GDK_BACKEND=x11
  # Private Xvfb, no shared desktop. Screenshot captures only this synthetic workspace.
  timeout 60 xvfb-run -a -s '-screen 0 1440x1000x24' bash -c '
    "$1" > "$2" 2>&1 & app_pid=$!
    echo "$DISPLAY" > "${2}.display"
    trap '\''kill "$app_pid" 2>/dev/null || true'\'' EXIT
    for i in {1..300}; do
      for page in hierarchy usage; do
        if grep -q "WORKSPACE_E2E capture=$page" "$2" && [ ! -e "${3%.png}-$page.png" ]; then import -window root "${3%.png}-$page.png"; fi
      done
      if grep -q "WORKSPACE_E2E phase=" "$2" && [ ! -e "$3" ]; then import -window root "$3"; fi
      kill -0 "$app_pid" 2>/dev/null || break
      sleep .1
    done
    wait "$app_pid"
  ' _ "$bin" "$log" "$run_dir/$phase.png" || { tail -40 "$log"; exit 1; }
  grep 'WORKSPACE_E2E' "$log"
  grep -qx 'WORKSPACE_E2E result=pass' "$log"
  test -s "$run_dir/$phase.png"
done
grep -qx 'WORKSPACE_E2E docs-denied=55' "$run_dir/restart.log"
echo "workspace GUI/restart checks: PASS ($run_dir)"
