# Tauri AppImage design note (revision 2, 2026-11-16)

Status: decided a Tauri AppImage with the server rewritten in Rust and the system WebKit. The Rust core (`core/`) is written and its 15 integration tests pass on this machine; the Tauri shell, the AppImage and the GUI tests are not written yet. Everything below marked TEST that is not in `core/tests/parity.rs` is still a promise, not a result. 

## 1. Tauri configuration (explicit)
- Pinned Tauri version (exact `=` requirement, recorded in `Cargo.lock`), `default-features = false`, explicit feature list; no `devtools` feature.
- `tauri.conf.json`: `withGlobalTauri: false`, no `assetProtocol`, no `dangerousRemoteDomainIpcAccess`, no `windows` entries (the window is built in code), explicit `security.csp`.
- Empty capability set: no `capabilities/` entries, no plugins (shell, fs, http, opener, dialog, clipboard, updater all absent), no `invoke_handler`.
- Window built in code with `on_navigation` allowing exactly the origin `viewer://localhost` (scheme, host `localhost`, no port, no credentials), nothing else. New windows: decided now: they are denied. If the pinned Tauri has `on_new_window`, it returns deny; if not, the build fails its check (below) and I do not ship until another way is found.
- TEST (headless under xvfb if it runs here, otherwise marked not run): `invoke` from the page fails; links to https, file and viewer opened with `target=_blank` do nothing; navigating away is blocked.

## 2. URI handling and CSP
- No listener. `viewer://` requests call `handle(config, path_and_query, sec_fetch_site)`. GET only; any other method is refused.
- Routes are exact matches: `/`, `/view`, `/img`, `/gate`, `/diff`. `//etc`, `/view/../view`, `/view/` and similar give 404. TEST.
- Query: decoded exactly once. A raw `p=`/`a=`/`b=` value containing `%00`, `%2e` or `%2f` (any case) is refused (our own links keep `/` and `.` literal, so no legitimate link needs them). Invalid UTF-8 after decoding is refused. A decoded value with a `..` component is refused. A repeated `p` is refused (`?p=a&p=b`). TEST for each.
- Headers on every response as in the Python viewer: nosniff, no-store, no-referrer, `Cross-Origin-Resource-Policy: same-origin`, CSP `default-src 'none'; style-src 'unsafe-inline'; img-src 'self'` (pages) and `default-src 'none'; sandbox` (images).
- Sec-Fetch-Site: TEST what WebKitGTK actually sends for custom-scheme subresources. Until measured I rely on defence in depth, in this order: (a) navigation locked to `viewer://localhost` and no external content ever loaded, so no other origin can issue the request; (b) the path policy; (c) the Sec-Fetch-Site check (`cross-site`, `same-site` refused). A missing header is accepted; I do not claim the check protects anything until the measurement shows it is sent.

## 3. Paths
- Policy decision: canonicalise, deny rules on every component (case-insensitive glob), allow list, regular file. Component-wise comparison (`Path::starts_with`). Non-UTF-8 names refused. NUL refused.
- Open: not `canonicalize` then plain `open`. The canonical path is re-opened relative to its allow-list root with `openat2` using `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS`, then `fstat`, regular file, same device and inode as the checked path, `O_NONBLOCK` set. This is the only `unsafe` code, in one module `sys.rs`; the crate has `#![deny(unsafe_code)]` with a single `#[allow]` on that module. If `openat2` is unavailable (kernel older than 5.6), the app refuses to read rather than fall back.
- Redaction: same token-shape regex, applied to text, diffs, gate rows and not-shown refs; `(?i)` applies to the whole pattern as in Python.

## 4. Configuration file
- `~/.config/agent-app/allowlist.toml`, edited by the the maintainer only. `AGENT_APP_CONFIG` exists in debug builds only and is compiled out of release builds. `gate_file` lives here too.
- Built-in default deny list (work, game, library, build, .config, .ssh, .gnupg, .env, credentials, token, key and certificate extensions, console and disc formats). The file can only add to it. (The Python viewer had this fail-open flaw, verified: a missing `deny_names` gave an empty list. It is now fixed there too, with tests.)
- Relative allow entries are refused (Python: fixed). `~` expands only to `$HOME`.
- The config file is opened with `O_NOFOLLOW`, `fstat`: owner is the effective uid, no group or other write bit, same checks on its parent directory. Failing any of them: error page, nothing readable, no fallback. Missing: page saying so, nothing readable. Malformed: error page. Empty list: reads nothing. TEST each.

## 5. Dependencies and supply chain
- `flake.lock` and `Cargo.lock` committed before the first real build. Crates vendored (`cargo vendor` or nix `importCargoLock`) and the Rust binary built offline in the nix sandbox. `cargo deny` with `sources` limited to crates.io and a licence allow-list (MIT, Apache-2.0, BSD, ISC, Unicode, Zlib; MPL-2.0 as files only). `cargo audit`. CI runs `cargo test --locked`, audit and deny; every CI tool is pinned by version and checksum or action SHA.
- AppImage: assembled from the nix-built binary (and the desktop file and icon) by a pinned script in the repo, not by Tauri's bundler downloading `linuxdeploy` at build time; no WebKit or GTK libraries are bundled (section 6). No zsync or update information. A sha256 file is written, and a detached signature is made only with a key the maintainer provides (I do not create or store keys; until then the artifact is marked unsigned and a checksum is not authenticity).

## 6. WebKit: decided by the maintainer (2026-11-16)
**Decision reversed for the NixOS standalone build (2026-11-23):** a local AppImage that bundles WebKitGTK, GTK, glib and glibc is built by `scripts/build-appimage-nixos.sh` (see `docs/STANDALONE-NIXOS.md`). It is not published; publishing needs a separate licence decision for the LGPL parts. The portable Ubuntu build below is unchanged.

The portable AppImage does not bundle the browser engine. The host's WebKitGTK (4.1) is required, and keeping it patched is the distribution's job. The README and the AppImage's start-up message say so; the app exits with a clear message if the library is missing. This removes the frozen-engine risk. The cost: the AppImage is not fully standalone, and it needs a host with `libwebkit2gtk-4.1`. The build therefore does not bundle WebKit or GTK libraries.

## 7. Tests (parity plus new)
- The 54 Python cases become `cargo test` cases one for one, plus: percent-decoding variants, hardlink to a denied file, `/proc` and `/sys` allow entries refused, other HTTP methods, FIFO and device files, a file that grows past the cap while read, `(?i)` applied to the whole secret regex, image header slices with `.get(..)` (done in `images.rs`), config permission and ownership cases, duplicate `p`, `//etc`.
- Same synthetic inputs run through both implementations; results compared before the Python viewer is retired. Until then the Rust port is HYPOTHESIS.

## 8. Toolchain
- From nixpkgs via cache.nixos.org (rustc and cargo 1.99.0, webkitgtk 4.1 2.54.1, gtk3, libsoup 3.8, librsvg, openssl), pinned by the committed `flake.lock` (created; the dev shell, Xvfb and the e2e script use it). Crates from crates.io, verified by `Cargo.lock` checksums. `CARGO_HOME` and `target/` inside the project and git-ignored. Nothing outside the project and the nix store; no `nix-collect-garbage` by me; nothing deleted in shared-docs or other coordinators' folders.

## Revision 3 (2026-11-16): measured results and answers to the second review
Measured on this machine (NixOS, Tauri 2.12.6 pinned with `=`, tauri-build 2.7.5, WebKitGTK 2.54.1 from nixpkgs, Xvfb), by `scripts/e2e.sh` with the test-only `e2e` feature (never in release builds): PASS.
- `invoke` from the page is rejected (no capability); `window.__TAURI__` is undefined (`withGlobalTauri: false`). `__TAURI_INTERNALS__` still exists as an object, but no command is registered and none can be called.
- New windows (`window.open` to https, `file://`, `viewer://localhost/`, and a `target=_blank` link click) are all denied by `on_new_window`; navigation to https is blocked by `on_navigation` and the page stays alive.
- Sec-Fetch-Site: WebKitGTK sends NO such header for custom-scheme subresources (measured: `None`). So that check never fires on this stack; the defences are the locked navigation and the path policy. The check stays as defence in depth only.
- D2: the new-window check is `scripts/e2e.sh` (expects `new-window-denied` for the three URLs and the link). It is a release blocker: if Xvfb or the app cannot run the script, I do not ship and I say why.
- D4b: any repeated query key is refused (checked with a set); `+` decodes to a space, a backslash in a path is just a character (no Windows semantics).
- D6: the allow root is opened as a path handle with `O_PATH|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC`; a file allow entry is opened relative to its parent directory by name; a swapped root is caught because the root is canonicalised at load and the read re-checks device and inode of the final file against the policy check; `ENOSYS` and `EPERM` (seccomp) from `openat2` are treated alike: refuse to read, with a clear message; `EAGAIN` is retried 3 times.
- D7: the config is opened `O_NOFOLLOW|O_NONBLOCK|O_CLOEXEC`, read with a cap+1 check (over 1 MB is an error, not a cut), its parent must be a directory owned by you and not group or other writable. `AGENT_APP_CONFIG` exists only under `debug_assertions` in `src-tauri/src/main.rs`; a CI check that the release binary lacks the string is still to be added.
- Allow entries refused at load: relative, non-existent (no fallback to an unresolved path), `/`, system roots, `/proc` `/sys` `/dev`, `$HOME` and its parents. Wrongly typed `allow`, `deny_names` or `gate_file` is an error, not ignored.
- Default deny list extended (.aws, .kube, .netrc, .npmrc, .git-credentials, id_*, kdbx, pgp). A test fails if the Rust and Python lists differ. The bare names work, game, library, build deny any allow root under such a folder; that is intended.
- Error text is one message for every refusal ("path not available").
- D8b: not done yet. The AppImage runtime and `appimagetool` will be named and pinned (nix store paths) and the relocation step described when the AppImage is built; nothing is built yet.
- D9/D10: hardlinks are refused (`st_nlink > 1`) and tested. The AppImage declares the host `libwebkit2gtk-4.1` and GTK 3 requirement in its README and start-up message; it is not fully standalone.

## Revision 4 (2026-11-16)
- A7: `compile_error!` stops the `e2e` feature in any build without debug assertions; `scripts/check-release-binary.sh` checks the release binary for `AGENT_APP_CONFIG`, `__e2e` and `__report`.
- A12: `scripts/e2e.sh` now has pipefail, checks the app's exit status, asserts `__TAURI_INTERNALS__` is an object, `__TAURI__` is undefined, `invoke` rejected, each new-window denial separately (the link and `window.open` to https use different URLs), the navigation block, and that Sec-Fetch-Site is absent (it fails if that changes). It uses the flake's tools, not the floating registry. It runs the debug build and is not in CI.
- A5: `on_navigation` also requires no password. A8: a relative config path is refused. A10: a failed response build becomes a 500. A3: CSP gains `frame-ancestors 'none'`, `form-action 'none'`, `base-uri 'none'` in the Rust core, the Tauri config and the Python viewer.
- Not changed: the diff timeout is not proven to fire by a test; two time-bound tests keep loose limits.

## Revision 5 (2026-11-16): the AppImage, as built
Chosen: build in an Ubuntu 22.04 container. Files: `packaging/Containerfile` (base image pinned by digest `sha256:45e9ddc6…`; apt packages not version-pinned), `packaging/AppRun`, `packaging/agent-app.desktop`, `scripts/build-appimage.sh`, `scripts/test-appimage.sh`, `packaging/Containerfile.test`.
- Build: the container mounts `/nix` read-only and uses the flake's cargo and rustc (version from `flake.lock`) with the offline, locked crates in `.cargo-home`; linking uses Ubuntu's gcc and WebKitGTK 2.50.4 headers. Result: dynamic binary, interpreter `/lib64/ld-linux-x86-64.so.2`, highest glibc symbol `GLIBC_2.34`, no `/nix/store` string, no missing libraries in the container, passes `check-release-binary.sh`.
- Packaging: AppDir with `AppRun`, desktop file, icon; squashfs (gzip, fixed timestamps) plus the type2 runtime `20251108` (`runtime-x86_64`, sha256 `2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d`, which equals the digest GitHub shows for that release asset; downloaded over https and refused on mismatch). That runtime is third-party binary code I did not build from source and whose signature I did not verify. No zsync or update information. A `.sha256` file is written; the image is UNSIGNED.
- The host must provide WebKitGTK 4.1 and GTK 3. `AppRun` lists any missing libraries and exits 1 with install hints.
- Tested (`scripts/test-appimage.sh`, PASS): in an Ubuntu 22.04 container with WebKitGTK and Xvfb the AppImage, run with `--appimage-extract-and-run`, starts and shows the docs list from the config; in a bare Ubuntu 22.04 container it prints the missing-library message and exits 1.
- Not tested: running through FUSE (rootless containers have none), any distro other than Ubuntu 22.04, Wayland, a real desktop session, a glibc older than 2.34 (the AppImage will not run there), reproducibility of the compile step (the packaging step gave identical sha256 on three runs; the binary was compiled once and reused).

## Revision 6 (2026-11-16): AppImage review follow-up
- AT1: `scripts/test-appimage.sh` now asserts the process is still alive after 8 s and that the screenshot contains link-blue pixels (fraction 0.00068 on a good run; 0 for a crashed or blank window). Negative control: with the AppImage replaced by a script that exits at once, the test FAILS (not running, no blue pixels, no missing-library message).
- BN1: the compile run and both test containers use `--network=none`. BN2: the build and test images carry a hash of their Containerfile(s) as a label and are rebuilt when it changes. BN5: `.cargo-home` was filled earlier by cargo, not by a script; `Cargo.lock` checksums are what make that fetch verifiable.
- AR2: `README.md` now says FUSE 3 is needed to run the AppImage normally and that only `--appimage-extract-and-run` was tested. AR3: licence texts for the crates and the runtime are NOT in the artifact yet; the README says so; required before any public release. The runtime's tag commit (dd6cebed, per the release page) goes next to the pin when the repo goes public, with an offer to build it from source.
- Reproducibility: after the build image was rebuilt (label change) the binary was recompiled from scratch in a fresh container and the AppImage sha256 was identical (`8269456a…`). One observation on this machine with the same inputs, not a proof.
