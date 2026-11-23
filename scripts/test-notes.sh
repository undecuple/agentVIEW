#!/usr/bin/env bash
# Notes library and export/import bundle in the real webview. Synthetic text only.
set -euo pipefail
cd "$(dirname "$0")/.."
run=${1:-$(mktemp -d /tmp/agentview-notes.XXXXXX)}
mkdir -p "$run";run=$(realpath "$run")
test ! -e "$run/data" || { echo 'Choose a fresh evidence directory';exit 2; }
mkdir -m700 "$run/data" "$run/config"
printf 'allow = []\n' > "$run/config/allowlist.toml";chmod 600 "$run/config/allowlist.toml"
export AGENT_APP_DATA_DIR="$run/data" AGENT_APP_CONFIG="$run/config/allowlist.toml" AGENT_APP_TEST_NOTES=1 AGENT_APP_TEST_PHASE=first
export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 GDK_BACKEND=x11
timeout --kill-after=8 60 xvfb-run -a -s '-screen 0 1440x1000x24' "$PWD/target/debug/agent-app" > "$run/notes.log" 2>&1 || { tail -30 "$run/notes.log";exit 1; }
grep -E 'E2E (check|error|result)' "$run/notes.log" | sed 's/%20/ /g'
grep -qx 'WORKSPACE_E2E result=pass' "$run/notes.log"
echo "Notes and bundle: PASS ($run)"
