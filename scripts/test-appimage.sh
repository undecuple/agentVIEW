#!/usr/bin/env bash
# Compatibility entrypoint for the current agentVIEW artifact checks.
set -euo pipefail
exec bash "$(dirname "$0")/test-agentview-appimage.sh" "$@"
