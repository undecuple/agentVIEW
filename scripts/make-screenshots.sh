#!/usr/bin/env bash
# Regenerates the preview screenshots from a throwaway profile with synthetic data only (needs ImageMagick `import`).
# Build first: cargo build -p agent-app --features workspace-e2e
set -euo pipefail
cd "$(dirname "$0")/.."
run=$(mktemp -d /tmp/agentview-shots.XXXXXX)
mkdir -m700 "$run/data" "$run/config"; printf 'allow = []\n' > "$run/config/allowlist.toml"; chmod 600 "$run/config/allowlist.toml"
printf '{"projects":{}}' > "$run/claude.json"
# Synthetic board, queue and parked questions so the Flow and Overview pages show something (all invented, written before the app starts)
n=$(date +%s); mkdir -m700 "$run/data/kanban" "$run/data/queues" "$run/data/decisions"
{ printf '{"t":%s,"id":"T-1","col":"active","title":"Write the parser tests","by":"worker-1","note":"half done"}\n' "$((n-300))"
  printf '{"t":%s,"id":"T-2","col":"queued","title":"Update the docs","by":"worker-2"}\n' "$((n-200))"
  printf '{"t":%s,"id":"T-3","col":"blocked","title":"Publish the release","by":"worker-3","note":"waiting for approval"}\n' "$((n-100))"
  printf '{"t":%s,"id":"T-4","col":"review","title":"Fix the login bug","by":"worker-1"}\n' "$((n-60))"
  printf '{"t":%s,"id":"T-5","col":"done","title":"Add the settings page","by":"worker-2"}\n' "$((n-30))"; } > "$run/data/kanban/example-app.jsonl"
{ printf '{"t":%s,"id":"run-1","ev":"running","who":"example-app","est_min":20,"slot":"0"}\n' "$((n-600))"
  printf '{"t":%s,"id":"w-1","ev":"queued","who":"example-tools","est_min":5,"pos":1,"since":%s,"note":"quick check"}\n' "$n" "$((n-120))"
  printf '{"t":%s,"id":"w-2","ev":"queued","who":"example-site","est_min":40,"pos":2,"since":%s}\n' "$n" "$((n-240))"
  printf '{"t":%s,"id":"res","ev":"reserved","who":"owner","note":"quiet window for a timing run","until":%s}\n' "$n" "$((n+900))"; } > "$run/data/queues/gpu.jsonl"
printf '{"t":%s,"id":"Q-1","ev":"ask","f":"worker-1","p":"example-app","q":"Use the new parser or keep the old one?","o":["New parser","Old parser"]}\n' "$((n-60))" > "$run/data/decisions/pending.jsonl"
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
