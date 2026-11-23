#!/usr/bin/env bash
# The release binary must not contain the debug-only config override or the e2e page and report endpoint.
set -u
bin=${1:-target/release/agent-app}
[ -f "$bin" ] || { echo "build first: cargo build --release -p agent-app"; exit 2; }
fail=0
for s in AGENT_APP_CONFIG AGENT_APP_DATA_DIR AGENT_APP_TEST_PHASE AGENT_APP_TEST_WIDTH AGENT_APP_TEST_CLAUDE AGENT_APP_TEST_CODEX AGENT_APP_TEST_LIVE AGENT_APP_TEST_PROJECT_ROOT AGENT_APP_TEST_LAYOUT AGENT_APP_TEST_CANCEL AGENT_APP_TEST_SETTINGS AGENT_APP_TEST_THEME __e2e __report __workspace-test __workspace-report __workspace-close __workspace-cancel-status; do
  if grep -aqF -- "$s" "$bin"; then echo "FOUND in release binary: $s"; fail=1; fi
done
echo "release binary check: $([ $fail = 0 ] && echo PASS || echo FAIL)"
exit $fail
