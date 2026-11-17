# agent-app

A read-only viewer for shared docs, gate status, diffs and images, as a small desktop app (Tauri 2 shell, Rust core).
It never grants an approval, redacts values that look like credentials (a pattern list, not a guarantee), and reads only files on an explicit allow-list.

Prepared with agent assistance.

Status: early. First public release will be source only; no AppImage is published yet. Not yet done: `cargo audit` and `cargo deny`, and the end-to-end and release-binary checks in CI. Unsigned builds, no release yet. Tested on one Linux machine only (extract-and-run mode, X11 via Xvfb).

## Requirements (AppImage)
- Linux x86_64 with glibc 2.34 or newer.
- WebKitGTK 4.1 and GTK 3 from your distribution (not bundled; keeping them patched is the distribution's job).
  Debian/Ubuntu: `libwebkit2gtk-4.1-0 libgtk-3-0`. Fedora: `webkit2gtk4.1 gtk3`. Arch: `webkit2gtk-4.1 gtk3`.
- To run an AppImage normally the host needs FUSE 3 (`fusermount3`). Without FUSE run it with `--appimage-extract-and-run`.
  Only the extract-and-run mode has been tested.
- Config: `~/.config/agent-app/allowlist.toml`, owned by you, not group or other writable, in a directory with the same
  rule. See `allowlist.example.toml`.

## Build and test
See `docs/DESIGN-tauri.md`. In short: `nix develop`, `cargo test -p agent-app-core`, `python3 -I -m unittest discover -s tests`,
`cargo build -p agent-app --features e2e && scripts/e2e.sh`, `scripts/build-appimage.sh`, `scripts/test-appimage.sh`.
The AppImage is checked with a sha256 file. It is not signed.

## Third-party code
See `NOTICE.md`. Licence texts for the Rust crates and the AppImage runtime are in `licenses/`. The AppImage contains the type2 runtime (release 20251108, pinned by sha256 `2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d`), a third-party binary that is not built from source here.

## Licence
MIT, see `LICENSE`.

## Known issues
- Dependabot alert GHSA-wrw7-89jp-8q8g (medium): unsoundness in `glib::VariantStrIter` iterators. `glib` 0.18.5 comes in through the Tauri and GTK dependencies and the fix is in `glib` 0.20 or later, which needs a Tauri update. This project's own code does not use `VariantStrIter` (searched the sources); whether the dependencies call it is not checked.
