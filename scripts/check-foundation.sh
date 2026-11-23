#!/usr/bin/env bash
# One local entrypoint, compact output, complete per-check logs. Run inside nix develop.
set -euo pipefail
cd "$(dirname "$0")/.."
out=${1:-"$(mktemp -d /tmp/agent-app-foundation.XXXXXX)"}
mkdir -p "$out"; out=$(realpath "$out")
export CARGO_HOME="$PWD/.cargo-home"
run() {
  local name=$1; shift
  if "$@" >"$out/$name.log" 2>&1; then
    echo "PASS $name"
  else
    echo "FAIL $name ($out/$name.log)"; tail -45 "$out/$name.log"; return 1
  fi
}
run python python3 -I -W error -m unittest discover -s tests
run icon python3 scripts/make-icon.py --check
run art python3 scripts/make-readme-assets.py --check
run rust bash scripts/check-rust.sh
run diagnostics-unit cargo test --locked --offline -p agent-app settings::tests
run viewer-build cargo build --locked --offline -p agent-app --features e2e
run viewer-policy bash scripts/e2e.sh
run workspace-build cargo build --locked --offline -p agent-app --features workspace-e2e
run workspace-restart bash scripts/test-workspace.sh "$out/gui"
run workspace-narrow env AGENT_APP_TEST_WIDTH=640 bash scripts/test-workspace.sh "$out/gui-narrow"
run live-cancel bash scripts/test-live-cancel.sh "$out/live-cancel"
run live-sessions bash scripts/test-live.sh "$out/live"
run live-narrow env AGENT_APP_TEST_WIDTH=640 bash scripts/test-live.sh "$out/live-narrow"
run app-settings bash scripts/test-settings.sh "$out/settings"
run app-settings-narrow env AGENT_APP_TEST_WIDTH=640 bash scripts/test-settings.sh "$out/settings-narrow"
run normal-build cargo build --locked --offline -p agent-app
echo "Foundation checks passed. Logs and previews: $out"
