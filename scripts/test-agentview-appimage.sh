#!/usr/bin/env bash
# Release-artifact smoke checks in network-disabled Ubuntu containers and a private Xvfb.
# Usage: bash scripts/test-appimage.sh [image] [fresh-output-directory]
# FUSE is not available in these containers: extract-and-run only.
set -euo pipefail
cd "$(dirname "$0")/.."
revision=$(git rev-parse --short=12 HEAD)
version=$(python3 scripts/release.py version)
img=$(realpath "${1:-dist-appimage/agentVIEW-$version-$revision-x86_64.AppImage}")
shot_dir=${2:-"$(mktemp -d /tmp/agentview-appimage.XXXXXX)"}
mkdir -p "$shot_dir"; shot_dir=$(realpath "$shot_dir")
test -x "$img"
for marker in appimage-workspace.png alive database-created; do
  test ! -e "$shot_dir/$marker" || { echo "Choose a fresh evidence directory" >&2; exit 2; }
done
fail=0
userns=${AGENT_APP_CONTAINER_USERNS:-keep-id}
cf=$(sha256sum packaging/Containerfile.test packaging/Containerfile | sha256sum | cut -d' ' -f1)
have=$(podman image inspect agent-app-test:jammy --format '{{index .Config.Labels "cf-sha"}}' 2>/dev/null || true)
[ "$have" = "$cf" ] || podman build --label "cf-sha=$cf" -t agent-app-test:jammy -f packaging/Containerfile.test packaging
echo "== (a) with WebKitGTK"
podman run --rm --network=none --userns="$userns" -v "$img":/app/agentVIEW.AppImage:ro -v "$shot_dir":/shots -e HOME=/tmp/h agent-app-test:jammy bash -c '
  set -eu
  mkdir -p /tmp/h/.local/share/agent-app /tmp/h/.config/agent-app
  chmod 700 /tmp/h/.local/share/agent-app /tmp/h/.config/agent-app
  printf "allow = []\n" > /tmp/h/.config/agent-app/allowlist.toml
  chmod 600 /tmp/h/.config/agent-app/allowlist.toml
  export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 GDK_BACKEND=x11
  xvfb-run -a -s "-screen 0 1440x1000x24" bash -c '\''
    /app/agentVIEW.AppImage --appimage-extract-and-run >/tmp/app.log 2>&1 & pid=$!
    trap "kill $pid 2>/dev/null || true" EXIT
    sleep 8
    kill -0 "$pid" 2>/dev/null && touch /shots/alive
    import -window root /shots/appimage-workspace.png
    test -s /tmp/h/.local/share/agent-app/workspace.sqlite3 && touch /shots/database-created
  '\''
  grep -v "MESA\|DRI3\|AT-SPI" /tmp/app.log | head -8 || true
' || { echo "FAIL: GUI container exited unsuccessfully"; fail=1; }
[ -e "$shot_dir/alive" ] || { echo "FAIL: app not running after 8 s"; fail=1; }
[ -e "$shot_dir/database-created" ] || { echo "FAIL: app-owned database absent"; fail=1; }
if [ -s "$shot_dir/appimage-workspace.png" ]; then
  if frac=$(python3 - "$shot_dir/appimage-workspace.png" <<'PYRENDER'
import runpy,sys
check=runpy.run_path('scripts/niri-smoke.py',run_name='agentview_render_check')['check_render']
print(check(sys.argv[1]))
PYRENDER
  ); then echo "Cyberdream dark accent-pixel fraction: $frac"; else echo "FAIL: themed workspace rendering absent"; fail=1; fi
else echo "FAIL: no workspace screenshot"; fail=1; fi
echo "== (b) without WebKitGTK"
set +e
out=$(podman run --rm --network=none -v "$img":/app/agentVIEW.AppImage:ro docker.io/library/ubuntu@sha256:45e9ddc6895604674618a409ca9f73d301da8fdb75fe330e8e9a27b3b219e4ad /app/agentVIEW.AppImage --appimage-extract-and-run 2>&1)
st=$?
set -e
printf '%s\n' "$out" | sed -n '1,8p'; echo "exit=$st"
{ [ "$st" != 0 ] && printf '%s\n' "$out" | grep -q 'which are missing'; } || { echo "FAIL: missing-library message absent"; fail=1; }
echo "AppImage smoke test: $([ "$fail" = 0 ] && echo PASS || echo FAIL) ($shot_dir)"
exit "$fail"
