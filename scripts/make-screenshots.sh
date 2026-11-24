#!/usr/bin/env bash
# Regenerates the preview screenshots from a throwaway profile with synthetic data only (needs ImageMagick `import`).
# Build first: cargo build -p agent-app --features workspace-e2e
set -euo pipefail
cd "$(dirname "$0")/.."
run=$(mktemp -d /tmp/agentview-shots.XXXXXX)
mkdir -m700 "$run/data" "$run/config"; printf 'allow = []\n' > "$run/config/allowlist.toml"; chmod 600 "$run/config/allowlist.toml"
printf '{"projects":{}}' > "$run/claude.json"
export AGENT_APP_DATA_DIR="$run/data" AGENT_APP_CONFIG="$run/config/allowlist.toml" AGENT_APP_TEST_SHOTS=1 AGENT_APP_TEST_PHASE=first AGENT_APP_TEST_CLAUDE_JSON="$run/claude.json"
export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 GDK_BACKEND=x11
xvfb-run -a -s '-screen 0 1440x900x24' bash -c '
  timeout 90 "$0/target/debug/agent-app" > "$1/app.log" 2>&1 &
  done=0
  for i in $(seq 1 600); do
    sleep 0.2
    while read -r name; do
      [ -f "$1/$name.png" ] || { import -window root "$1/$name.png"; echo "captured $name"; }
    done < <(grep -o "shot=[a-z-]*" "$1/app.log" | cut -d= -f2)
    grep -q "result=" "$1/app.log" && break
  done
  wait || true
' "$PWD" "$run"
grep -q 'result=pass' "$run/app.log" || { tail -20 "$run/app.log"; exit 1; }
echo "screenshots in $run"
