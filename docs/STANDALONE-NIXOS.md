# Standalone AppImage for NixOS (local build)

Why: the portable AppImage needs the host's WebKitGTK 4.1 and GTK 3, which a NixOS host does not provide in the usual places. This build carries its own copy so it starts on a bare NixOS. Decision of 2026-11-23, reversing "system WebKit" for this build only. Local build only: it contains LGPL libraries, and publishing it needs a separate licence decision.

Build: commit first, then `bash scripts/build-appimage-nixos.sh` (needs `nix` and network once for the pinned, hash-checked runtime). Output: `dist-appimage/agentVIEW-nixos-<version>-<commit>-x86_64.AppImage` plus a `.sha256` (integrity only, unsigned). About 310 MB.

How it works: the image holds the Nix closure of the binary (about 190 store paths: WebKitGTK, GTK, glib, glibc, fonts, icons, schemas, bubblewrap). `AppRun` uses bubblewrap to lay them over `/nix/store` in a private user namespace, then starts the app with the bundled loader. The host root, home and your Claude Code or Codex CLIs stay visible; nothing on the host changes.

Needs on the host: unprivileged user namespaces (the NixOS default) and X11 or Wayland. GPU drivers come from the host; if the window stays black, start with `AGENTVIEW_SOFTWARE_RENDER=1`. Not verified on a real GPU session by the author: please test it on your desktop.

Test: `bash scripts/test-appimage-nixos.sh <image>` starts it under Xvfb with the host store hidden (proves the bundle is complete) and again overlaid, with a throwaway HOME.

Licence: `usr/share/licenses/agentVIEW/BUNDLED-LIBRARIES.txt` inside the image lists every bundled package; sources are the nixpkgs revision pinned in `flake.lock`.
