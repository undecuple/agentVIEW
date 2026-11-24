#!/usr/bin/env bash
# Read-only Claude config view against a synthetic file. Real ~/.claude.json is never read.
set -euo pipefail
cd "$(dirname "$0")/.."
run=${1:-$(mktemp -d /tmp/agentview-ccfg.XXXXXX)}
mkdir -p "$run";run=$(realpath "$run")
test ! -e "$run/data" || { echo 'Choose a fresh evidence directory';exit 2; }
mkdir -m700 "$run/data" "$run/config" "$run/proj"
printf 'allow = []\n' > "$run/config/allowlist.toml";chmod 600 "$run/config/allowlist.toml"
cat > "$run/claude.json" <<JSON
{"userID":"synthetic-user","oauthAccount":{"emailAddress":"synthetic-address-value"},
 "mcpServers":{"deploy-tool":{"command":"/usr/bin/deploy-tool","args":["--flag-arg","SECRET-TOKEN"],"env":{"API_KEY":"sk-synthetic-0000"}}},
 "projects":{"$run/proj":{"hasTrustDialogAccepted":true,"allowedTools":["Bash(ls)"],"history":[{"display":"private words"}],"mcpServers":{}},
             "$run/gone":{"hasTrustDialogAccepted":false,"allowedTools":[],"mcpServers":{"web":{"type":"http","url":"https://tools.example.test/mcp","headers":{"Authorization":"Bearer SECRET-TOKEN"}}}}}}
JSON
tok="Bearer abcdefghijkl""mnopqrstuvwxyz012345"   # assembled at run time
mkdir -p "$run/claudedir/skills/demo-skill" "$run/claudedir/commands" "$run/claudedir/agents"
cat > "$run/claudedir/settings.json" <<JSON
{"permissions":{"allow":["Bash(git status)","Bash(curl -H 'Authorization: $tok' x)"],"deny":["Read(./.env)"],"defaultMode":"acceptEdits"},
 "hooks":{"Notification":[{"matcher":"permission_prompt","hooks":[{"type":"command","command":"/usr/bin/notify-tool --token SECRET-HOOK-ARG"}]}]},
 "env":{"API_TOKEN":"SECRET-ENV-VALUE"},"model":"synthetic-model","apiKeyHelper":"/bin/helper SECRET-HELPER"}
JSON
printf -- '---\nname: demo-skill\ndescription: Summarises synthetic text\n---\nBody SECRET-BODY\n' > "$run/claudedir/skills/demo-skill/SKILL.md"
printf -- '---\ndescription: Deploy the synthetic thing\n---\nrun SECRET-BODY\n' > "$run/claudedir/commands/deploy.md"
printf -- '---\nname: reviewer\ndescription: Reviews synthetic code\nmodel: synthetic-small\ntools: Read, Grep\n---\nSECRET-BODY\n' > "$run/claudedir/agents/reviewer.md"
export AGENT_APP_DATA_DIR="$run/data" AGENT_APP_CONFIG="$run/config/allowlist.toml" AGENT_APP_TEST_CCFG=1 AGENT_APP_TEST_PHASE=first AGENT_APP_TEST_CLAUDE_JSON="$run/claude.json" AGENT_APP_TEST_CLAUDE_DIR="$run/claudedir"
export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 GDK_BACKEND=x11
timeout --kill-after=8 60 xvfb-run -a -s '-screen 0 1440x1000x24' "$PWD/target/debug/agent-app" > "$run/ccfg.log" 2>&1 || { tail -30 "$run/ccfg.log";exit 1; }
grep -E 'E2E (check|error|result)' "$run/ccfg.log" | sed 's/%20/ /g'
grep -qx 'WORKSPACE_E2E result=pass' "$run/ccfg.log"
echo "Claude config view: PASS ($run)"
