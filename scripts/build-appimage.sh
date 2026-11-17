#!/usr/bin/env bash
# Builds the AppImage. Steps: (1) compile in the pinned Ubuntu 22.04 container with the flake's Rust toolchain,
# (2) check the binary, (3) assemble AppDir, squashfs and the pinned type2 runtime, (4) write a sha256 file.
# Needs: podman, nix develop (flake.lock), network only for the one-time runtime download (hash-checked) and the
# image build. The compile and the checks run with --network=none. The crates in .cargo-home were fetched earlier by
# cargo (not by this script); Cargo.lock checksums are what make that fetch verifiable.
# Unsigned: a detached signature is made only with a key the maintainer provides.
set -euo pipefail
cd "$(dirname "$0")/.."
version=0.1.0
epoch=${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct)}
runtime_url=https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64
runtime_sha=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
out=dist-appimage; rm -rf "$out"; mkdir -p "$out" target-jammy

# 1. container build (toolchain from /nix, mounted read-only; crates from the locked, offline .cargo-home)
# Rebuild the build image whenever the Containerfile changes (the image carries its hash as a label).
cf=$(sha256sum packaging/Containerfile | cut -d' ' -f1)
have=$(podman image inspect agent-app-build:jammy --format '{{index .Config.Labels "cf-sha"}}' 2>/dev/null || true)
[ "$have" = "$cf" ] || podman build --label "cf-sha=$cf" -t agent-app-build:jammy -f packaging/Containerfile packaging
tc=$(nix develop --command bash -c 'echo $(dirname $(command -v cargo)):$(dirname $(command -v rustc))' 2>/dev/null | tail -1)
podman run --rm --network=none --userns=keep-id -v /nix:/nix:ro -v "$PWD":/work:ro -v "$PWD/target-jammy":/out -v "$PWD/.cargo-home":/cargo \
  -e CARGO_HOME=/cargo -e CARGO_TARGET_DIR=/out -e SOURCE_DATE_EPOCH="$epoch" \
  -e PATH="$tc:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" -w /work agent-app-build:jammy \
  cargo build --release --locked --offline -p agent-app
bin=target-jammy/release/agent-app

# 2. binary checks: no nix paths, no test-only strings, no missing libs in the container, glibc floor
scripts/check-release-binary.sh "$bin"
! grep -aqF /nix/store "$bin" || { echo "binary mentions /nix/store"; exit 1; }
podman run --rm --network=none -v "$PWD/target-jammy":/out:ro agent-app-build:jammy sh -c \
  'test "$(ldd /out/release/agent-app | grep -c "not found")" = 0 && objdump -T /out/release/agent-app | grep -o "GLIBC_[0-9.]*" | sort -Vu | tail -1' \
  | sed 's/^/highest glibc symbol needed: /'

# 3. AppDir, squashfs, runtime
d=$(mktemp -d); trap 'rm -rf "$d"' EXIT
appdir="$d/AppDir"; mkdir -p "$appdir/usr/bin"
install -m 755 "$bin" "$appdir/usr/bin/agent-app"
install -m 755 packaging/AppRun "$appdir/AppRun"
install -m 644 packaging/agent-app.desktop "$appdir/agent-app.desktop"
install -m 644 src-tauri/icons/icon.png "$appdir/agent-app.png"
ln -s agent-app.png "$appdir/.DirIcon"
mkdir "$d/rt"; curl -fsSL --proto '=https' -o "$d/rt/runtime" "$runtime_url"
echo "$runtime_sha  $d/rt/runtime" | sha256sum -c - >/dev/null || { echo "runtime hash mismatch, refusing"; exit 1; }
env -u SOURCE_DATE_EPOCH mksquashfs "$appdir" "$d/app.squashfs" -root-owned -noappend -comp gzip -mkfs-time "$epoch" -all-time "$epoch" -quiet
img="$out/agent-app-$version-x86_64.AppImage"
cat "$d/rt/runtime" "$d/app.squashfs" > "$img"; chmod 755 "$img"

# 4. checksum (integrity only; not a signature)
( cd "$out" && sha256sum "$(basename "$img")" > "$(basename "$img").sha256" )
cat "$out"/*.sha256
