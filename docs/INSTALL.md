# Installing agentVIEW

agentVIEW is a Tauri 2 desktop app for Linux (x86_64). There is no published binary yet: you build it from source, either to run straight from the source folder or to make an AppImage. Pick the section for your system.

- [What you need](#what-you-need)
- [Any Linux with Nix](#any-linux-with-nix)
- [Linux without Nix](#linux-without-nix)
- [NixOS](#nixos) (standalone AppImage, launcher command, menu entry)
- [First run](#first-run)
- [Updating and removing](#updating-and-removing)
- [Troubleshooting](#troubleshooting)

## What you need

- A Linux desktop session (Wayland or X11). Other platforms are not supported.
- The [Claude Code](https://docs.claude.com/en/docs/claude-code) and/or Codex CLI, installed and logged in the way you normally do. agentVIEW starts these programs for you in its own terminals; it does not install, update or log in to them.
- Git, to get the source.

## Any Linux with Nix

The repository pins its toolchain and libraries with a flake, so this works on any distribution that has Nix with flakes enabled.

```sh
git clone https://github.com/undecuple/agentVIEW.git
cd agentVIEW
nix develop                                   # pinned Rust toolchain, WebKitGTK 4.1, GTK 3
CARGO_HOME="$PWD/.cargo-home" cargo run --locked -p agent-app
```

That runs the app from the source folder. To make a release binary instead: `cargo build --release --locked -p agent-app`, which writes `target/release/agent-app`. A release binary built inside `nix develop` is linked against Nix store paths, so on a non-NixOS system prefer the AppImage below.

## Linux without Nix

Install a stable Rust toolchain (rustup is fine), `pkg-config`, a C compiler, and the development packages for WebKitGTK 4.1, GTK 3, libsoup 3, librsvg and OpenSSL:

| Distribution | Packages |
| --- | --- |
| Debian, Ubuntu | `build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev libssl-dev` |
| Fedora | `gcc pkgconf-pkg-config webkit2gtk4.1-devel gtk3-devel libsoup3-devel librsvg2-devel openssl-devel` |
| Arch | `base-devel webkit2gtk-4.1 gtk3 libsoup3 librsvg openssl` |

Then, from a clone of the repository:

```sh
cargo build --release --locked -p agent-app
./target/release/agent-app
```

To install it for your user: copy `target/release/agent-app` to `~/.local/bin/` and, if you want a menu entry, copy `packaging/agent-app.desktop` to `~/.local/share/applications/` and point its `Exec=` line at the binary.

A portable AppImage (needs WebKitGTK 4.1, GTK 3 and glibc 2.34 or newer on the machine that runs it) is built with `bash scripts/build-appimage.sh`. See `docs/RELEASING.md`. AppImages are unsigned.

## NixOS

On NixOS the ordinary AppImage does not start, because it expects WebKitGTK and GTK in the usual system places. The repository therefore has a standalone build that carries its own copy of everything it needs (WebKitGTK, GTK, glib, fonts, icons, bubblewrap). Details are in `docs/STANDALONE-NIXOS.md`. You need flakes enabled and unprivileged user namespaces (the NixOS default).

### 1. Build the AppImage

```sh
git clone https://github.com/undecuple/agentVIEW.git
cd agentVIEW
nix develop -c bash scripts/build-appimage-nixos.sh
```

- The script refuses to run with uncommitted changes, so it can label the image with the exact commit. If you edited files, commit them first.
- It needs network once to fetch a pinned, hash-checked AppImage runtime.
- The first build takes several minutes. The output is `dist-appimage/agentVIEW-nixos-<version>-<commit>-x86_64.AppImage` plus a `.sha256` file.

### 2. Put it in place

```sh
cd dist-appimage
sha256sum -c agentVIEW-nixos-*.AppImage.sha256        # must say OK
mkdir -p ~/Applications
cp agentVIEW-nixos-*.AppImage ~/Applications/agentVIEW.AppImage
chmod +x ~/Applications/agentVIEW.AppImage
~/Applications/agentVIEW.AppImage                     # first run
```

Keeping one fixed file name (`agentVIEW.AppImage`) means updating is just copying a new build over it.

### 3. Add an `agentview` command and a menu entry

Add this to `configuration.nix` (or a module it imports), then run `sudo nixos-rebuild switch`. It only creates a small launcher; it does not build or download the app.

```nix
{ pkgs, ... }:
let
  agentview = pkgs.writeShellScriptBin "agentview" ''
    exec "$HOME/Applications/agentVIEW.AppImage" "$@"
  '';
  agentviewDesktop = pkgs.makeDesktopItem {
    name = "agentview";
    desktopName = "agentVIEW";
    comment = "Local workspace for Claude Code and Codex sessions";
    exec = "agentview";
    categories = [ "Development" ];
  };
in
{
  environment.systemPackages = [ agentview agentviewDesktop ];
}
```

With Home Manager, put the same two values in `home.packages` instead.

If you would rather not rebuild the system, a symlink works for the command: `ln -s ~/Applications/agentVIEW.AppImage ~/.local/bin/agentview` (make sure `~/.local/bin` is on your `PATH`), and a `~/.local/share/applications/agentview.desktop` file with `Exec=` pointing at the AppImage gives you the menu entry.

### 4. Just trying it, without installing

`nix develop` then `CARGO_HOME="$PWD/.cargo-home" cargo run --locked -p agent-app` runs the app straight from the source folder (see above). Nothing is installed, and it is the quickest way to try a change.

### NixOS notes

- The image is self-contained, so `nix-collect-garbage` does not break it.
- GPU drivers come from the host. If the window stays black, start it with `AGENTVIEW_SOFTWARE_RENDER=1 agentview`.
- Builds from late November 2026 onward include a fix for Wayland sessions that report no screen resolution (the symptom was a squashed, unreadable layout). If you see that on an older build, rebuild.
- Your Claude Code and Codex CLIs run inside the app's terminals with your normal environment. Make sure they are on your `PATH` (for example through `environment.systemPackages` or Home Manager).

## First run

- **App data** is kept in `~/.local/share/agent-app` (or under `$XDG_DATA_HOME`), created private (mode 0700). It holds the app's database, exports and notes. Nothing leaves your machine unless you set up a webhook or a backup yourself.
- **Document viewer allow-list** (optional): copy `allowlist.example.toml` to `~/.config/agent-app/allowlist.toml` and list the files you want the read-only viewer to show. Without it the viewer shows nothing, and the rest of the app works.
- **Sessions:** open Sessions, choose Claude Code or Codex, press Inspect, register a project folder, then Start. Login and permission prompts stay in the terminal.
- **Optional:** Settings, Notifications lets you add a webhook (for example for a phone message when an agent may be waiting) and shows a Claude Code hook you can paste into your own Claude settings so the app knows when Claude is waiting for a decision. The app never edits your Claude settings.

## Updating and removing

- **Update:** get the new source (`git pull`), build again and replace the file you installed.
- **Remove:** delete the binary or AppImage, the launcher (`~/.local/bin/agentview`, the `.desktop` file, or the Nix snippet), and, only if you want to lose your notes and history, `~/.local/share/agent-app` and `~/.config/agent-app`.

## Troubleshooting

| Symptom | What to try |
| --- | --- |
| `Commit the candidate first` when building the NixOS image | Commit or stash your changes; the script will not label a modified tree with a commit id. |
| Black window | Start with `AGENTVIEW_SOFTWARE_RENDER=1`. |
| Squashed layout, text one word per line | Rebuild from a current commit (see NixOS notes). |
| The AppImage will not start on NixOS | Use the NixOS build above, not the portable one. Check that unprivileged user namespaces are enabled. |
| `claude` or `codex` not found in Sessions | Install the CLI and make sure it is on the `PATH` of the shell or session that launches agentVIEW. |
| Build fails on missing `webkit2gtk-4.1` or `libsoup-3.0` | Install the development packages listed for your distribution, or use `nix develop`. |

Found a problem with this guide? Please open an issue.
