#!/usr/bin/env bash
# AppImage or test-only native fixture, native Wayland, installed Niri; isolated inside a task-owned Xvfb.
# Run within nix develop .#niri: bash scripts/test-niri.sh [--native-fixture] IMAGE [FRESH_OUTPUT_DIR]
set -euo pipefail
cd "$(dirname "$0")/.."
exec timeout --kill-after=20 120 xvfb-run -a -s '-screen 0 1440x1000x24' python3 scripts/niri-smoke.py "$@"
