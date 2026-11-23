#!/usr/bin/env bash
# Starts the standalone AppImage twice (host Nix store hidden, then overlaid) under Xvfb with a throwaway HOME,
# and checks the window process stays up. Software rendering is forced because Xvfb has no GPU. Usage: test-appimage-nixos.sh IMAGE
set -euo pipefail
img=$(realpath "${1:?AppImage path}"); t=$(mktemp -d /tmp/agentview-ai.XXXXXX); trap 'rm -rf "$t"' EXIT
( cd "$t" && "$img" --appimage-extract >/dev/null 2>&1 ) || { echo "extract failed"; exit 1; }
for iso in 1 0; do
  mkdir -p "$t/home$iso"
  env AGENTVIEW_ISOLATE_STORE=$iso AGENTVIEW_SOFTWARE_RENDER=1 GDK_BACKEND=x11 HOME="$t/home$iso" timeout 40 xvfb-run -a -s '-screen 0 1280x900x24' bash -c '
    "$1" >"$2" 2>&1 & pid=$!; sleep 12
    kill -0 $pid 2>/dev/null || { echo "exited early"; cat "$2"; exit 1; }
    import -window root "$3"; kill $pid' _ "$t/squashfs-root/AppRun" "$t/log$iso" "$t/shot$iso.png"
  python3 - "$t/shot$iso.png" <<'P'
import subprocess,sys
out=subprocess.run(["convert",sys.argv[1],"-format","%[fx:mean]","info:"],capture_output=True,text=True).stdout
assert float(out)>0.03,"window is black"
P
  echo "isolate_store=$iso: window rendered"
done
echo "standalone AppImage start: PASS"
