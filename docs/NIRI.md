# Niri compatibility checks

The workspace uses normal Wayland windows and makes no assumptions about window
position. Its measured Wayland app ID is `agent-app`, matching the existing
desktop file. No compositor configuration or global shortcut changes are needed.
At widths below 800 pixels, Documents and Usage remain available as labelled
icon buttons; the agent hierarchy and conversation adapt to the narrower column.

## Reproduce the isolated smoke

```sh
nix develop .#niri --command bash scripts/test-niri.sh \
  dist-appimage/agentVIEW-VERSION-COMMIT-x86_64.AppImage /tmp/agentview-niri-fresh
```

The optional shell supplies Niri and its nested X11 backend libraries. To check
an installed Niri instead, set `NIRI_BINARY` to its absolute executable path.
The existing `agent-app-test:jammy` Podman image must have been created by the
AppImage smoke/build tooling. This harness uses rootless Podman with host user
namespace mapping, not host root, and disables the container network.

The harness creates its own Xvfb display, runtime directory, minimal Niri config,
D-Bus session, application data and allow-list. It never uses `niri --session`,
the desktop's IPC socket or the user's existing app database. Captures and logs
stay in the fresh output directory. SIGTERM/SIGINT trigger bounded cleanup of
only the recorded container and private compositor.

Checks cover native Wayland window creation, 55% and 100% tiling columns,
1.5 output scaling, persistent database creation and a rendered-content control.
Real RGB/RGBA image tests reject blank frames and invisible transparent accents.
The separate foundation GUI suite exercises visible Documents/Usage navigation
at 640 pixels and persisted conversation recovery at both window sizes.

## Evidence and limits

The installed Niri 26.04 has rendered the packaged app under this isolated
setup. An older preview exposed the missing narrow navigation, now corrected.
The full foundation suite passed 74 Python tests, 31 Rust integration tests and
both default/narrow GUI restart runs. Existing Python HTTP fixture cleanup emits
ResourceWarning messages; the passing suite is not warning-free.

This is software-rendered native Wayland inside nested Niri on private Xvfb.
The AppImage runs in an Ubuntu 22.04 container for its system WebKit/GTK libraries.
It does not establish direct AppImage loader support on NixOS, hardware rendering,
FUSE mounting, physical keyboard/IME/clipboard behavior, multiple monitors or
performance. Native Nix source builds remain available through the default shell.
No user desktop was captured or reconfigured.


## Live terminal resizing

Build `agent-app` with `--features workspace-e2e` in the pinned shell, then run:

```sh
nix develop .#niri --command bash scripts/test-niri.sh --native-fixture \
  target/debug/agent-app /tmp/agentview-niri-live-fresh
```

This mode runs the native debug application with two synthetic CLI sessions,
not the release AppImage or an authenticated provider. Everything remains in
private display/runtime/config/data directories. It covers 55% and full-width
columns, then 100%, 125%, 150% and 200% compositor scaling. After each resize,
the GUI verifies a positive-sized, fitted, fully visible terminal; the synthetic
child reports its actual kernel PTY rows/columns. A separate 640-pixel GUI run
checks input, controls and two-process restart; it is included in foundation checks.

The compact roster and collapsible launch details reduce vertical stacking.
When a focused terminal changes size, it scrolls into view and its controls remain accessible through page scrolling. Routine polling does not force scrolling. These checks do not establish
physical input/IME, mixed-monitor transitions or hardware-renderer performance.
