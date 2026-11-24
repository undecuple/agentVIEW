#!/usr/bin/env bash
# rclone backup page against a fake rclone program. Real rclone and real remotes are never used.
set -euo pipefail
cd "$(dirname "$0")/.."
run=${1:-$(mktemp -d /tmp/agentview-rclone.XXXXXX)}
mkdir -p "$run";run=$(realpath "$run")
test ! -e "$run/data" || { echo 'Choose a fresh evidence directory';exit 2; }
mkdir -m700 "$run/data" "$run/config"
printf 'allow = []\n' > "$run/config/allowlist.toml";chmod 600 "$run/config/allowlist.toml"
cat > "$run/fake-rclone" <<SH
#!/usr/bin/env bash
echo "\$@" >> "$run/argv.log"
case "\$1" in
  listremotes) printf 'mydrive:\nother:\n';;
  copy) for f in "\$2"/*.json; do n=\$(basename "\$f"); if [[ " \$* " == *" --dry-run "* ]]; then echo "2026/11/23 10:00:00 NOTICE: \$n: Skipped copy as --dry-run" >&2; else echo "2026/11/23 10:00:00 INFO  : \$n: Copied (new)" >&2; fi; done;;
  *) exit 3;;
esac
SH
chmod 700 "$run/fake-rclone"
export AGENT_APP_DATA_DIR="$run/data" AGENT_APP_CONFIG="$run/config/allowlist.toml" AGENT_APP_TEST_RCLONE="$run/fake-rclone" AGENT_APP_TEST_PHASE=first
export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 GDK_BACKEND=x11
timeout --kill-after=8 60 xvfb-run -a -s '-screen 0 1440x1000x24' "$PWD/target/debug/agent-app" > "$run/rclone.log" 2>&1 || { tail -30 "$run/rclone.log";exit 1; }
grep -E 'E2E (check|error|result)' "$run/rclone.log" | sed 's/%20/ /g'
grep -qx 'WORKSPACE_E2E result=pass' "$run/rclone.log"
# Only listremotes and copy (with --immutable, never sync/delete/move) may ever have been called.
! grep -Eq '(sync|delete|move|purge|config|--delete)' "$run/argv.log"
grep -c ' --immutable ' "$run/argv.log" >/dev/null
echo "rclone backup: PASS ($run)"
