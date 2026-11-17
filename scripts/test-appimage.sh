#!/usr/bin/env bash
# Tests the AppImage in containers (no network): (a) an Ubuntu 22.04 image WITH WebKitGTK starts it, the process is
# still alive after 8 s, and the screenshot shows link-blue pixels (a crash leaves a black root window, a blank
# WebKit view leaves white), (b) a bare Ubuntu 22.04 image WITHOUT WebKitGTK prints the missing-library message
# and exits non-zero. No FUSE in rootless containers, so the AppImage runs with --appimage-extract-and-run
# (the FUSE path is NOT tested here).
set -u
cd "$(dirname "$0")/.."
img=$(ls dist-appimage/*.AppImage); name=$(basename "$img"); shot_dir=${1:-dist-appimage/test}; mkdir -p "$shot_dir"
fail=0
cf=$(sha256sum packaging/Containerfile.test packaging/Containerfile | sha256sum | cut -d' ' -f1)
have=$(podman image inspect agent-app-test:jammy --format '{{index .Config.Labels "cf-sha"}}' 2>/dev/null || true)
[ "$have" = "$cf" ] || podman build --label "cf-sha=$cf" -t agent-app-test:jammy -f packaging/Containerfile.test packaging >/dev/null
rm -f "$shot_dir/appimage-docs.png" "$shot_dir/alive"
echo "== (a) with WebKitGTK"
podman run --rm --network=none --userns=keep-id -v "$PWD/dist-appimage":/app:ro -v "$PWD/$shot_dir":/shots -e HOME=/tmp/h agent-app-test:jammy bash -c '
  set -e; mkdir -p /tmp/h/docs /tmp/h/.config/agent-app; chmod 700 /tmp/h/.config/agent-app /tmp/h/.config
  printf "# Synthetic\n\nhello\n" > /tmp/h/docs/a.md
  printf "allow = [\"/tmp/h/docs\"]\n" > /tmp/h/.config/agent-app/allowlist.toml; chmod 600 /tmp/h/.config/agent-app/allowlist.toml
  export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1
  xvfb-run -a -s "-screen 0 1000x700x24" bash -c "
    /app/'"$name"' --appimage-extract-and-run >/tmp/app.log 2>&1 & pid=\$!
    sleep 8; kill -0 \$pid 2>/dev/null && touch /shots/alive
    import -window root /shots/appimage-docs.png; kill \$pid 2>/dev/null; true"
  grep -v "MESA\|DRI3\|AT-SPI" /tmp/app.log | head -5 || true'
[ -e "$shot_dir/alive" ] && echo "process alive after 8 s" || { echo "FAIL: app not running after 8 s"; fail=1; }
if [ -s "$shot_dir/appimage-docs.png" ]; then
  frac=$(magick "$shot_dir/appimage-docs.png" -fuzz 20% -fill black +opaque '#0000EE' -fill white -opaque '#0000EE' -format '%[fx:mean]' info:)
  echo "link-blue pixel fraction: $frac"
  awk -v f="$frac" 'BEGIN{exit !(f>0.0003)}' && echo "link pixels present" || { echo "FAIL: no link-blue pixels, window blank or crashed"; fail=1; }
else echo "FAIL: no screenshot"; fail=1; fi
echo "== (b) without WebKitGTK"
out=$(podman run --rm --network=none -v "$PWD/dist-appimage":/app:ro docker.io/library/ubuntu@sha256:45e9ddc6895604674618a409ca9f73d301da8fdb75fe330e8e9a27b3b219e4ad \
  /app/$name --appimage-extract-and-run 2>&1); st=$?
echo "$out" | head -4; echo "exit=$st"
{ [ "$st" != 0 ] && echo "$out" | grep -q "which are missing"; } && echo "message check ok" || { echo "FAIL: expected missing-library message and non-zero exit"; fail=1; }
echo "appimage test: $([ $fail = 0 ] && echo PASS || echo FAIL)"; exit $fail
