#!/usr/bin/env bash
# Claude Notification-hook signal -> webhook message, against a fake curl. Nothing leaves this machine.
set -euo pipefail
cd "$(dirname "$0")/.."
run=${1:-$(mktemp -d /tmp/agentview-hooksig.XXXXXX)}
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
export AGENT_APP_DATA_DIR="$run/data" AGENT_APP_CONFIG="$run/config/allowlist.toml" AGENT_APP_TEST_CURL="$run/fake-curl" AGENT_APP_TEST_HOOKSIG=1 AGENT_APP_TEST_POLL_MS=300 AGENT_APP_TEST_PHASE=first
export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 GDK_BACKEND=x11
sig="$run/data/signals/claude-notify.jsonl"
# Appends lines once the page reports ready: one old line is ignored by design (written before the app reads), then real ones.
( for i in $(seq 1 600); do grep -q 'ready=1' "$run/hook.log" 2>/dev/null && break; sleep 0.1; done
  printf '{"t":%s,"s":"sess-1","c":"/work/example-project","k":"permission_prompt","m":"Claude needs your permission to use Bash"}\n' "$(date +%s)" >> "$sig"
  printf '{"t":%s,"s":"sess-1","c":"/work/example-project","k":"permission_prompt","m":"Claude needs your permission to use Bash"}\n' "$(date +%s)" >> "$sig"
  printf '{"t":%s,"s":"sess-2","c":"/work/other","k":"auth_success","m":"signed in"}\n' "$(date +%s)" >> "$sig"
  printf 'not json at all\n' >> "$sig" ) &
timeout --kill-after=8 90 xvfb-run -a -s '-screen 0 1440x1000x24' "$PWD/target/debug/agent-app" > "$run/hook.log" 2>&1 || { tail -30 "$run/hook.log";exit 1; }
wait
grep -E 'E2E (check|error|result)' "$run/hook.log" | sed 's/%20/ /g'
grep -qx 'WORKSPACE_E2E result=pass' "$run/hook.log"
n=$(grep -c 'waiting for your decision' "$run/curl.log" || true)
[ "$n" = 1 ] || { echo "expected exactly one waiting message (duplicate and non-waiting lines ignored), got $n"; cat "$run/curl.log"; exit 1; }
grep -q 'example-project' "$run/curl.log" || { echo 'message does not name the agent'; exit 1; }
if grep -q '/work/example-project' "$run/curl.log"; then echo 'folder path leaked into the message'; exit 1; fi
if grep '^ARGS:' "$run/curl.log" | grep -q 'SECRET-TOKEN'; then echo 'address leaked into arguments'; exit 1; fi
echo "Hook signal: PASS ($run)"
