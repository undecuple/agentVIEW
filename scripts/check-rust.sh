#!/usr/bin/env bash
# Run inside the flake's dev shell; CI and local checks use the same locked crates.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo test --locked --workspace --exclude agent-app
cargo build --locked -p agent-app
# The app crate's own unit tests (git view, redaction, docshare, ...). The window-based GUI suites stay separate.
cargo test --locked -p agent-app --bins
