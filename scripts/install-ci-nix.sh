#!/usr/bin/env bash
# GitHub-only action policy: install official Nix directly, with pinned archive checksum.
# Run only on an ephemeral Linux CI runner; never invoked by local check scripts.
set -euo pipefail
[[ "${GITHUB_ACTIONS:-}" == true && "$(uname -sm)" == 'Linux x86_64' ]] || { echo 'CI Linux x86_64 runner required' >&2; exit 2; }
version=2.35.2
sha=0c3960a9792331a22081c3c7a5d8465db9b17c50b3acdf18587fa4c6f2cb1158
archive="${RUNNER_TEMP:?}/nix-$version.tar.xz"
curl --proto '=https' --tlsv1.2 -fsSL --retry 3 -o "$archive" "https://releases.nixos.org/nix/nix-$version/nix-$version-x86_64-linux.tar.xz"
printf '%s  %s\n' "$sha" "$archive" | sha256sum -c -
tar -xJf "$archive" -C "$RUNNER_TEMP"
"$RUNNER_TEMP/nix-$version-x86_64-linux/install" --no-daemon --no-channel-add --no-modify-profile --yes
printf '%s\n' "$HOME/.nix-profile/bin" >> "${GITHUB_PATH:?}"
printf '%s\n' 'NIX_CONFIG=experimental-features = nix-command flakes' >> "${GITHUB_ENV:?}"
"$HOME/.nix-profile/bin/nix" --version
