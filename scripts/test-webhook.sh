#!/usr/bin/env bash
# Webhook settings and the curl call, against a fake curl program. Nothing leaves this machine.
set -euo pipefail
cd "$(dirname "$0")/.."
run=${1:-$(mktemp -d /tmp/agentview-hook.XXXXXX)}
mkdir -p "$run";run=$(realpath "$run")
test ! -e "$run/data" || { echo 'Choose a fresh evidence directory';exit 2; }
mkdir -m700 "$run/data" "$run/config"
printf 'allow = []\n' > "$run/config/allowlist.toml";chmod 600 "$run/config/allowlist.toml"
cat > "$run/fake-curl" <<SH
#!/usr/bin/env bash
{ echo "ARGS: \$*"; echo "STDIN: \$(cat)"; } >> "$run/curl.log"
printf 204
SH
chmod 700 "$run/fake-curl"
export AGENT_APP_DATA_DIR="$run/data" AGENT_APP_CONFIG="$run/config/allowlist.toml" AGENT_APP_TEST_CURL="$run/fake-curl" AGENT_APP_TEST_PHASE=first
export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 GDK_BACKEND=x11
timeout --kill-after=8 60 xvfb-run -a -s '-screen 0 1440x1000x24' "$PWD/target/debug/agent-app" > "$run/hook.log" 2>&1 || { tail -30 "$run/hook.log";exit 1; }
grep -E 'E2E (check|error|result)' "$run/hook.log" | sed 's/%20/ /g'
grep -qx 'WORKSPACE_E2E result=pass' "$run/hook.log"
# The address goes to curl on stdin only, never in the arguments; the call uses https-only flags and no redirects.
grep -q '^STDIN: url = "https://hooks.example.test/api/SECRET-TOKEN-123"' "$run/curl.log"
if grep -q SECRET-TOKEN-456 "$run/curl.log"; then echo 'a destination that was not tested received a message'; exit 1; fi
if grep '^ARGS:' "$run/curl.log" | grep -q 'SECRET-TOKEN'; then echo 'address leaked into arguments'; exit 1; fi
grep '^ARGS:' "$run/curl.log" | grep -q -- '--proto =https'
grep '^ARGS:' "$run/curl.log" | grep -q -- '--max-redirs 0'
# The address is not written anywhere outside the private database; the log files do not contain it.
if grep -q 'SECRET-TOKEN' "$run/hook.log"; then echo 'address leaked into the log'; exit 1; fi
echo "Webhook: PASS ($run)"
