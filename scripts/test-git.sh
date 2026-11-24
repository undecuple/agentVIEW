#!/usr/bin/env bash
# Read-only git view against a synthetic repository. No real repository is read.
set -euo pipefail
cd "$(dirname "$0")/.."
run=${1:-$(mktemp -d /tmp/agentview-git.XXXXXX)}
mkdir -p "$run";run=$(realpath "$run")
test ! -e "$run/data" || { echo 'Choose a fresh evidence directory';exit 2; }
mkdir -m700 "$run/data" "$run/config" "$run/proj"
G="git -C $run/proj -c init.defaultBranch=main"
$G init -q;$G config user.name tester;$G config user.email 'tester@users.noreply.github.com'
echo one > "$run/proj/a.md";$G add a.md;GIT_CONFIG_GLOBAL=/dev/null $G commit -qm 'first synthetic commit'
echo two >> "$run/proj/a.md";$G add a.md
GIT_AUTHOR_EMAIL=other-id GIT_COMMITTER_EMAIL=other-id $G commit -qm 'second synthetic commit'
echo three >> "$run/proj/a.md";echo x > "$run/proj/new.txt";echo SECRET-VALUE > "$run/proj/.env"
printf 'allow = ["%s/a.md"]\n' "$run/proj" > "$run/config/allowlist.toml";chmod 600 "$run/config/allowlist.toml"
export AGENT_APP_DATA_DIR="$run/data" AGENT_APP_CONFIG="$run/config/allowlist.toml" AGENT_APP_TEST_GIT=1 AGENT_APP_TEST_PHASE=first
export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 GDK_BACKEND=x11
timeout --kill-after=8 60 xvfb-run -a -s '-screen 0 1440x1000x24' "$PWD/target/debug/agent-app" > "$run/git.log" 2>&1 || { tail -30 "$run/git.log";exit 1; }
grep -E 'E2E (check|error|result)' "$run/git.log" | sed 's/%20/ /g'
grep -qx 'WORKSPACE_E2E result=pass' "$run/git.log"
echo "Git view: PASS ($run)"
