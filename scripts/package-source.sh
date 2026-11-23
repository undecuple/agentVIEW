#!/usr/bin/env bash
# Exact committed sources only: excludes config, caches, database and local build outputs.
set -euo pipefail
cd "$(dirname "$0")/.."
revision=$(git rev-parse --short=12 HEAD)
out=${AGENT_APP_DIST_DIR:-dist-appimage}
mkdir -p "$out"
name="agentVIEW-$revision-source.tar.gz"
git archive --format=tar --prefix="agentVIEW-$revision/" HEAD | gzip -n > "$out/$name"
gzip -t "$out/$name"
(cd "$out" && sha256sum "$name" > "$name.sha256")
cat "$out/$name.sha256"
