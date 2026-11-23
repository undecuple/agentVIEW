# Third-party components and credits

agent-app is MIT licensed (see `LICENSE`). It is not affiliated with or endorsed by Anthropic, OpenAI or any other
company whose tools it may be used alongside. It contains no content from any game, no keys and no credentials.

| Component | Use | Licence |
|---|---|---|
| [Tauri](https://tauri.app/) 2 and its Rust dependencies | desktop shell and the Rust core | MIT or Apache-2.0 (and others per crate; full list in `licenses/THIRD-PARTY.md`, texts in `licenses/rust-crates-*.txt`) |
| `cssparser`, `cssparser-macros`, `dtoa-short`, `option-ext`, `selectors` | transitive Rust crates | MPL-2.0 (text in `licenses/rust-crates-*.txt`; used unmodified) |
| [AppImage type2 runtime](https://github.com/AppImage/type2-runtime) (release 20251108) | prepended to the AppImage; pinned by sha256, third-party binary, not built here | MIT (`licenses/runtime/type2-runtime-MIT.txt`) |
| libfuse, statically linked into the runtime | mounts the AppImage | LGPL-2.1 (`licenses/runtime/libfuse-LGPL-2.txt`) |
| squashfuse, statically linked into the runtime | reads the image | BSD-2-Clause (`licenses/runtime/squashfuse-LICENSE.txt`) |
| zstd, zlib, statically linked into the runtime | decompression | BSD / zlib (`licenses/runtime/zstd-LICENSE.txt`) |
| WebKitGTK 4.1 and GTK 3 | web view; taken from the host system in the portable build. The local NixOS standalone build bundles them (with glib, glibc, fonts and icons; list in `BUNDLED-LIBRARIES.txt` inside the image) | LGPL-2.1+. The standalone build is local only and is not distributed; publishing it needs a separate licence decision (licence texts, source offer) |
| `rusqlite` 0.32.1 and `libsqlite3-sys` 0.30.1 | workspace persistence (bundled SQLite) | MIT bindings; SQLite is public domain; added dependency texts in `licenses/workspace-crates.txt` |
| [xterm.js](https://github.com/xtermjs/xterm.js) 6.0.0 and first-party addon-fit 0.11.0 | vendored embedded terminal; pinned integrity/hashes in `ui/vendor/manifest.json`, MIT texts in `licenses/terminal/` | MIT |
| [undecuple/me](https://github.com/undecuple/me) README artwork construction | double-rule frames and dithered dividers adapted for agentVIEW's original generated lettering and hierarchy artwork | MIT (same project copyright holder) |

What the runtime statically links was read from the runtime's own licence file; the exact libraries in the pinned
binary have not been inspected. Source for the runtime is at the project linked above. Crate versions are fixed by
`Cargo.lock`. The `dlopen2` crates ship no licence file in their source; their declared licence is in the table.
