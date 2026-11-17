#!/usr/bin/env bash
# Runs the e2e (debug) build of the app under Xvfb and asserts what the webview may and may not do.
# Run inside `nix develop` (flake.lock pins the toolchain, Xvfb and WebKitGTK). Not in CI yet: CI has no pinned nix setup.
# Build first: cargo build -p agent-app --features e2e
set -u -o pipefail
bin=target/debug/agent-app
[ -x "$bin" ] || { echo "build first: cargo build -p agent-app --features e2e"; exit 2; }
d=$(mktemp -d); chmod 700 "$d"
printf 'allow = ["%s"]\n' "$d" > "$d/allowlist.toml"; chmod 600 "$d/allowlist.toml"
log="$d/out.log"
WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 GDK_BACKEND=x11 \
AGENT_APP_CONFIG="$d/allowlist.toml" \
  timeout 60 xvfb-run -a "$bin" >"$log" 2>&1
status=$?
grep -E 'E2E' "$log"
fail=0
[ "$status" = 0 ] || { echo "FAIL: app exit status $status (expected 0 from the done report)"; fail=1; }
need() { grep -qF -- "$1" "$log" || { echo "MISSING: $1"; fail=1; }; }
need_line() { grep -qxF -- "$1" "$log" || { echo "MISSING LINE: $1"; fail=1; }; }
refuse() { grep -qF -- "$1" "$log" && { echo "UNEXPECTED: $1"; fail=1; }; }
need 'report done=1'
need 'report tauri_internals=object'
need 'report tauri_global=undefined'
need 'report invoke=rejected'
refuse 'invoke=RESOLVED'
refuse 'returned-window'
need 'new-window-denied https://example.com/link'      # the target=_blank link click
need_line 'E2E new-window-denied https://example.com/'   # window.open https (whole line, so the /link case cannot satisfy it)
need 'new-window-denied file:///etc/passwd'
need 'new-window-denied viewer://localhost/'
need 'navigation blocked https://example.com/'
need 'report still_here_after_nav=yes'                  # navigation blocked, so the page is still alive
# Sec-Fetch-Site: measured as absent for custom-scheme subresources; fail if that ever changes so the note gets updated
need 'E2E sec-fetch-site=None'
echo "e2e result: $([ $fail = 0 ] && echo PASS || echo FAIL)"
exit $fail
