#!/usr/bin/env bash
# The release binary must not contain the debug-only config override or the e2e page and report endpoint.
set -u
bin=${1:-target/release/agent-app}
[ -f "$bin" ] || { echo "build first: cargo build --release -p agent-app"; exit 2; }
fail=0
for s in AGENT_APP_CONFIG __e2e __report; do
  if grep -aqF -- "$s" "$bin"; then echo "FOUND in release binary: $s"; fail=1; fi
done
echo "release binary check: $([ $fail = 0 ] && echo PASS || echo FAIL)"
exit $fail
