#!/usr/bin/env bash
# Standalone AppImage for NixOS and other Linux hosts: bundles the Nix closure (WebKitGTK, GTK, glib, glibc,
# fonts, icons) so the host needs only bubblewrap-capable user namespaces. LOCAL BUILD ONLY: it contains LGPL
# libraries, so publishing waits for a separate licence decision. Unsigned. Needs network once for the pinned runtime.
set -euo pipefail
cd "$(dirname "$0")/.."
[ -z "$(git status --porcelain)" ] || { echo "Commit the candidate first; refusing to label a dirty build with HEAD" >&2; exit 2; }
version=$(python3 scripts/release.py version); revision=$(git rev-parse --short=12 HEAD)
epoch=${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct)}
runtime_url=https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64
runtime_sha=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
out=${AGENT_APP_DIST_DIR:-dist-appimage}; mkdir -p "$out"
nix develop --command bash -c 'CARGO_HOME=$PWD/.cargo-home cargo build --release --locked --offline -p agent-app'
bin=target/release/agent-app
scripts/check-release-binary.sh "$bin"
extras=$(nix build --no-link --print-out-paths .#standalone-extras); fonts=$(nix build --no-link --print-out-paths .#fonts-conf)
interp=$(nix develop --command readelf -l "$bin" | sed -n 's/.*interpreter: \(.*\)\]/\1/p')
bw="$extras/bin/bwrap"; [ -x "$bw" ] || { echo "bwrap missing from extras" >&2; exit 1; }
d=$(mktemp -d); trap 'chmod -R u+w "$d" 2>/dev/null; rm -rf "$d"' EXIT
app="$d/AppDir"; mkdir -p "$app/nix/store" "$app/usr/bin" "$app/usr/share"
{ ldd "$bin" | grep -o '/nix/store/[^ /]*'; ldd "$bw" | grep -o '/nix/store/[^ /]*'; echo "$extras"; echo "$fonts"; echo "$(dirname "$(dirname "$interp")")"; } | sort -u > "$d/roots"
xargs -a "$d/roots" nix-store -qR | sort -u > "$d/closure"
xargs -a "$d/closure" cp -a --reflink=auto -t "$app/nix/store/"
chmod -R u+w "$app/nix/store"
rel() { echo "${1#/nix/store/}"; }
bwlibs=$(ldd "$bw" | grep -o '/nix/store/[^ ]*' | xargs -n1 dirname | sort -u | while read -r p; do rel "$p"; done | tr '\n' ' ')
cat > "$app/usr/share/agentVIEW-paths.env" <<P
INTERP=$(rel "$interp")
BWRAP=$(rel "$(readlink -f "$bw")")
BWRAP_LIBS="$bwlibs"
EXTRAS=$(rel "$extras")
FONTS=$(rel "$fonts")
P
install -m 755 "$bin" "$app/usr/bin/agent-app"; install -m 755 packaging/AppRun-nixos "$app/AppRun"
install -m 644 packaging/agent-app.desktop "$app/agent-app.desktop"; install -m 644 src-tauri/icons/icon.png "$app/agent-app.png"; ln -s agent-app.png "$app/.DirIcon"
lic="$app/usr/share/licenses/agentVIEW"; mkdir -p "$lic"; cp -R licenses/. "$lic/"
install -m 644 LICENSE NOTICE.md assets/cyberdream-LICENSE.txt "$lic/"
{ echo "Libraries bundled in this build (Nix store paths; sources are the nixpkgs revision pinned in flake.lock):"; sed 's#^/nix/store/[a-z0-9]*-##' "$d/closure" | sort -u; } > "$lic/BUNDLED-LIBRARIES.txt"
printf 'agentVIEW standalone (NixOS)\nsource commit: %s\nunsigned local build; not for publication until the licence decision\n' "$(git rev-parse HEAD)" > "$app/usr/share/agentVIEW-build.txt"
mkdir "$d/rt"; curl -fsSL --proto '=https' -o "$d/rt/runtime" "$runtime_url"
echo "$runtime_sha  $d/rt/runtime" | sha256sum -c - >/dev/null || { echo "runtime hash mismatch, refusing"; exit 1; }
nix develop --command env -u SOURCE_DATE_EPOCH mksquashfs "$app" "$d/app.squashfs" -root-owned -noappend -comp zstd -Xcompression-level 19 -mkfs-time "$epoch" -all-time "$epoch" -quiet
img="$out/agentVIEW-nixos-$version-$revision-x86_64.AppImage"
cat "$d/rt/runtime" "$d/app.squashfs" > "$img"; chmod 755 "$img"
( cd "$out" && sha256sum "$(basename "$img")" > "$(basename "$img").sha256" )
ls -l "$img"
