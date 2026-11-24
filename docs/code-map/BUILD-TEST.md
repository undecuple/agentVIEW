# Build and test map

Run Nix-based commands from the repository root inside the development shell, for example `nix develop -c bash -c "bash scripts/check-rust.sh"`.

## Which command for which change

| You changed | Run |
|---|---|
| Rust in `core`, `sessions` or `workspace` | `bash scripts/check-rust.sh` (all workspace tests, then a build of the app) |
| A Tauri command, a page script, HTML or CSS | rebuild with `cargo build -p agent-app --features workspace-e2e`, then the matching GUI script below. The test JavaScript is embedded at compile time, so always rebuild before re-running. |
| Layout or text size | `scripts/test-overlap.sh` (every page at 80, 100, 125 and 150 percent) |
| Icon or README art | `python3 scripts/make-icon.py` / `make-readme-assets.py`, then the same with `--check` |
| Python, scripts or the viewer | `python3 -I -W error -m unittest discover -s tests` |
| Anything before a pull request | `python3 scripts/pre-pr-check.py --repo . --base <rev> --body <file>`, then the manual review page |
| Release or packaging | `bash scripts/check-foundation.sh` (the full local gate) |

## Scripts

- **Aggregates and static checks:** `check-foundation.sh` (16 steps: Python tests, icon and art checks, Rust checks, the sandbox e2e, the workspace, live and settings GUI tests, a normal build), `check-rust.sh`, `check-history.sh` (author and committer identity, forbidden message lines, file scan per commit), `scan-files.sh <rev>` (reads NUL-separated paths on stdin; blocks keys, archives, databases, files over 1 MiB and secret-shaped text), `check-release-binary.sh` (the release binary must contain none of the test hooks), `e2e.sh` (webview sandbox assertions).
- **GUI tests** (real WebKit under Xvfb; debug build with `--features workspace-e2e`; each uses a fresh evidence directory and synthetic data): `test-git.sh` (synthetic repository), `test-rclone.sh` (fake rclone program), `test-webhook.sh` (fake curl program), `test-hook-signal.sh` (Claude hook signal to a webhook, fake curl), `make-screenshots.sh` (regenerates the preview PNGs from synthetic data), `test-workspace.sh` (twice against one database; also the denied-command probe), `test-settings.sh`, `test-live.sh` and `test-live-cancel.sh` (use `tests/live-cli-fixture.py` as a fake CLI), `test-review.sh`, `test-notes.sh`, `test-claude-config.sh` (synthetic state file with fake secrets), `test-overlap.sh`.
- **CI helper:** `install-ci-nix.sh` installs a pinned Nix in CI runners (aborts anywhere else).
- **Packaging:** `build-appimage.sh` (portable build in a container), `build-appimage-nixos.sh` (standalone NixOS build, local only), `test-agentview-appimage.sh`, `test-appimage.sh`, `test-appimage-nixos.sh`, `package-source.sh`, `test-niri.sh` with `niri-smoke.py`.
- **Python tools:** `release.py`, `pre-pr-check.py`, `init-repo.py`, `make-icon.py`, `make-readme-assets.py`.

`check-foundation.sh` does not run the notes, review, Claude config or overlap GUI tests; run those yourself when you touch those pages.

## Test environment variables

All `AGENT_APP_TEST_*` variables are read only by debug builds with the test features (the release check fails if their names appear in a release binary): `PHASE` (`first`, `restart`, `corrupt`, `future`), `WIDTH`, `LIVE`, `CANCEL`, `CLAUDE`, `CODEX`, `PROJECT_ROOT`, `LAYOUT`, `THEME`, `SETTINGS`, `REVIEW`, `NOTES`, `OVERLAP`, `CCFG`, `CLAUDE_JSON`, `SCREEN`. `AGENT_APP_DATA_DIR` and `AGENT_APP_CONFIG` redirect storage in debug builds only. Packaging tests also use `AGENTVIEW_ISOLATE_STORE` and `AGENTVIEW_SOFTWARE_RENDER`.

The two Cargo features are `e2e` (sandbox test page and report endpoint) and `workspace-e2e` (implies `e2e`; adds `workspace_e2e.rs` and the embedded test scripts). Both write to the same debug binary, so build them one after the other.

## Test counts (at the time of writing)

Python 78 tests in 5 files. Rust: `core` 26, `workspace` 22, `sessions` 35, `src-tauri` 3. Eight embedded JavaScript GUI test files in `tests/`.

## Nix, packaging and CI

- `flake.nix`: a default development shell (Rust, Python, Xvfb, ImageMagick, WebKitGTK and GTK libraries), a `niri` shell, and packages `fonts-conf` and `standalone-extras` for the standalone build. nixpkgs follows `nixos-unstable`, pinned by `flake.lock`.
- `packaging/`: `AppRun` (portable launcher), `AppRun-nixos` (Nix-store launcher using bubblewrap), the desktop entry and two container files.
- `.github/workflows/ci.yml`: one `checks` job on every push and pull request: history and file scan, the Python tests, then `check-rust.sh` in the Nix shell. It does not run the GUI scripts.
- `.github/workflows/release.yml`: on version tags or manual dispatch; runs `check-foundation.sh`, builds the source archive and AppImage, tests it, and can create a **draft** release only.
- `.githooks/pre-commit` scans staged files with `scan-files.sh`. Enable once with `git config core.hooksPath .githooks`.
