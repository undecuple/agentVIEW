# Release workflow

`.github/workflows/release.yml` builds committed source and the Linux x86_64
AppImage, runs source tests and generated-art checks, smoke-tests the packaged
app in Ubuntu containers, and verifies checksums and provenance.
The full foundation helper is a release gate, including the actual viewer
navigation/new-window checks and workspace command-isolation/restart GUI tests.

## Manual validation

Actions → Release → Run workflow builds the selected revision and retains a
seven-file Actions artifact for 14 days. A manual run **never creates a GitHub
release**, including when run against a tag. The build job has contents read
permission only. Downloads include the tarball, AppImage, individual checksum
files, `SHA256SUMS`, `build-provenance.json` and `RELEASE_NOTES.md`.

GitHub-owned actions are pinned by commit to match this repository's policy.
The official Nix installer is pinned by archive checksum. Cargo uses the lockfile;
compile and smoke-test containers have no network. Container bootstrap packages
come from Ubuntu and are not version-pinned, as documented for the existing
packaging toolchain. Public crate sources and native/portable build outputs are cached;
provider credentials and app state are never cached.

## Version tags create drafts

After a maintainer authorizes a version tag, a push matching `v*` starts the same
build. The tag must match the versions in all three Rust package manifests and
the Tauri configuration, and point to a commit contained in `main`. A mismatched,
unrelated, dirty or unexpected source revision is refused.

Only that tag-push path enables a separate job with contents write permission.
It downloads the tested artifact and revalidates exact file names, source
identity, checksums and release notes before running `gh release create` with
`--verify-tag --draft`. Existing releases are never overwritten. The workflow
does not create tags, publish drafts, or mark a release latest automatically.

Review the draft and publish it as a separate authorized action. No tag or release
is created just by adding or merging this workflow.

## Current limits

AppImages are unsigned and need host WebKitGTK 4.1, GTK 3 and glibc 2.34 or newer.
Smoke tests use extract-and-run; FUSE and Wayland remain unverified. The preview
still uses simulated agents. Dependency audit/deny remain separate unfinished
checks. `scripts/release.py` has adversarial tests for tag validation, main
ancestry, manual-run refusal, checksum/provenance tampering and release reuse.
