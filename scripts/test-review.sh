#!/usr/bin/env bash
# Manual-review view in the real webview, two runs against one database; the draft changes between runs. Synthetic only.
set -euo pipefail
cd "$(dirname "$0")/.."
run=${1:-$(mktemp -d /tmp/agentview-review.XXXXXX)}
mkdir -p "$run";run=$(realpath "$run")
test ! -e "$run/data" || { echo 'Choose a fresh evidence directory';exit 2; }
mkdir -m700 "$run/data" "$run/config" "$run/drafts"
cat > "$run/drafts/pr.md" <<'MD'
## Why
The failure and its cause. <script>window.pwned=1</script>

[a link](javascript:alert(1)) and **bold** text.

| Item | Result |
|---|---|
| tests | run |

## Change
- one focused change
MD
printf 'allow = ["%s"]\n' "$run/drafts/pr.md" > "$run/config/allowlist.toml";chmod 600 "$run/config/allowlist.toml"
export AGENT_APP_DATA_DIR="$run/data" AGENT_APP_CONFIG="$run/config/allowlist.toml" AGENT_APP_TEST_REVIEW=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 GDK_BACKEND=x11
for phase in first restart; do
  [ "$phase" = restart ] && printf '\n## Evidence\nChanged after review.\n' >> "$run/drafts/pr.md"
  export AGENT_APP_TEST_PHASE="$phase"
  timeout --kill-after=8 60 xvfb-run -a -s '-screen 0 1440x1000x24' bash -c '
    "$1" > "$2" 2>&1 & pid=$!
    trap '\''kill "$pid" 2>/dev/null || true'\'' EXIT
    for i in {1..500};do
      for name in $(grep -o "capture=review-[a-z]*" "$2" | cut -d= -f2 | sort -u);do [ -e "$3/$name.png" ] || import -window root "$3/$name.png";done
      kill -0 "$pid" 2>/dev/null || break
      sleep .1
    done
    wait "$pid"
  ' _ "$PWD/target/debug/agent-app" "$run/$phase.log" "$run" || { tail -30 "$run/$phase.log";exit 1; }
  grep -E 'E2E (check|error|result)' "$run/$phase.log" | sed 's/%20/ /g'
  grep -qx 'WORKSPACE_E2E result=pass' "$run/$phase.log"
done
echo "Manual review view: PASS ($run)"
