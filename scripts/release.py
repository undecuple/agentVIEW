#!/usr/bin/env python3
"""Validate release identity and assets; tag pushes may create drafts, never publish.

Manual workflow runs only build. All external commands use argument arrays.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parent.parent
SEMVER = r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?"


def git(root, *args):
    return subprocess.run(["git", *args], cwd=root, check=True, text=True,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout.strip()


def version(root):
    values = [tomllib.loads((root / folder / "Cargo.toml").read_text())["package"]["version"]
              for folder in ["core", "workspace", "sessions", "src-tauri"]]
    values.append(json.loads((root / "src-tauri/tauri.conf.json").read_text())["version"])
    if len(set(values)) != 1 or not re.fullmatch(SEMVER, values[0]):
        raise ValueError("Package and Tauri versions must agree and be semantic versions")
    return values[0]


def metadata(root, env):
    if git(root, "status", "--porcelain"):
        raise ValueError("Release sources must be a clean committed tree")
    commit = git(root, "rev-parse", "HEAD")
    expected = env.get("GITHUB_SHA", commit)
    if not re.fullmatch(r"[0-9a-f]{40}", expected) or git(root, "rev-parse", expected + "^{commit}") != commit:
        raise ValueError("Checkout does not match the triggering commit")
    current_version = version(root)
    ref = env.get("GITHUB_REF", "local")
    event = env.get("GITHUB_EVENT_NAME", "local")
    tag = ""
    if ref.startswith("refs/tags/"):
        tag = ref.removeprefix("refs/tags/")
        if tag != "v" + current_version or not re.fullmatch("v" + SEMVER, tag):
            raise ValueError("Release tag must match all committed package versions")
        if git(root, "rev-parse", "refs/tags/" + tag + "^{commit}") != commit:
            raise ValueError("Tag target does not match checkout")
        git(root, "merge-base", "--is-ancestor", commit, "refs/remotes/origin/main")
    elif event == "push":
        raise ValueError("Release pushes must be version tags")
    if event not in ("push", "workflow_dispatch", "local"):
        raise ValueError("Unsupported release event")
    return {"commit": commit, "tree": git(root, "rev-parse", "HEAD^{tree}"),
            "revision": commit[:12], "version": current_version, "tag": tag,
            "ref": ref, "event": event, "draft_allowed": event == "push" and bool(tag)}


def names(meta):
    return [f"agentVIEW-{meta['revision']}-source.tar.gz",
            f"agentVIEW-{meta['version']}-{meta['revision']}-x86_64.AppImage"]


def digest(path):
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def check_artifacts(folder, meta):
    records = []
    for name in names(meta):
        path = folder / name
        if path.is_symlink() or not path.is_file() or not path.stat().st_size:
            raise ValueError("Missing or invalid artifact: " + name)
        sha_file = folder / (name + ".sha256")
        if sha_file.is_symlink() or sha_file.read_text().split() != [digest(path), name]:
            raise ValueError("Checksum mismatch: " + name)
        records.append({"name": name, "sha256": digest(path), "bytes": path.stat().st_size})
    return records


def prepare(root, folder, env):
    meta = metadata(root, env)
    records = check_artifacts(folder, meta)
    notes = (f"# agentVIEW {meta['tag'] or meta['revision']}\n\n"
             f"Source commit: `{meta['commit']}`\n\n"
             "Preview with simulated workspace records and explicitly launched local Claude Code and Codex terminals. Authenticated native-TUI recovery is awaiting verification.\n\n"
             "Assets: committed source tarball and unsigned Linux x86_64 AppImage. "
             "SHA-256 files check integrity; they are not signatures.\n\n"
             "The AppImage requires glibc 2.34+, system WebKitGTK 4.1 and GTK 3. "
             "Container smoke checks use `--appimage-extract-and-run`; FUSE is unverified. Wayland coverage is limited to isolated software-rendered Niri checks; see docs/NIRI.md.\n\n"
             "A version tag creates this as a draft. Review before publishing.\n")
    (folder / "RELEASE_NOTES.md").write_text(notes)
    provenance = dict(meta, files=records, notes_sha256=digest(folder / "RELEASE_NOTES.md"),
                      platform="linux-x86_64", signed=False)
    (folder / "build-provenance.json").write_text(json.dumps(provenance, indent=2, sort_keys=True) + "\n")
    checks = names(meta) + ["build-provenance.json", "RELEASE_NOTES.md"]
    (folder / "SHA256SUMS").write_text("".join(f"{digest(folder / n)}  {n}\n" for n in checks))
    verify(root, folder, env)
    return meta


def verify(root, folder, env):
    meta = metadata(root, env)
    expected = names(meta) + [n + ".sha256" for n in names(meta)] + ["build-provenance.json", "RELEASE_NOTES.md", "SHA256SUMS"]
    if set(p.name for p in folder.iterdir()) != set(expected):
        raise ValueError("Release directory must contain exactly the expected seven files")
    if any((folder / n).is_symlink() or not (folder / n).is_file() for n in expected):
        raise ValueError("Release files must be regular files, never symlinks")
    provenance = json.loads((folder / "build-provenance.json").read_text())
    if any(provenance.get(k) != v for k, v in meta.items()):
        raise ValueError("Artifact provenance does not match this event/commit")
    if provenance.get("files") != check_artifacts(folder, meta) or provenance.get("signed") is not False or provenance.get("platform") != "linux-x86_64":
        raise ValueError("Artifact identity mismatch")
    if provenance.get("notes_sha256") != digest(folder / "RELEASE_NOTES.md"):
        raise ValueError("Release notes changed after preparation")
    checks = names(meta) + ["build-provenance.json", "RELEASE_NOTES.md"]
    if (folder / "SHA256SUMS").read_text() != "".join(f"{digest(folder / n)}  {n}\n" for n in checks):
        raise ValueError("Aggregate checksums differ")
    return meta, [folder / n for n in expected]


def publish(root, folder, env):
    # Reject manual dispatch before any release API call, even if files are malicious.
    if env.get("GITHUB_EVENT_NAME") != "push" or not env.get("GITHUB_REF", "").startswith("refs/tags/v"):
        raise ValueError("Only a validated version-tag push may create a draft")
    meta, files = verify(root, folder, env)
    repo = env.get("GITHUB_REPOSITORY", "")
    if not meta["draft_allowed"] or not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repo):
        raise ValueError("Invalid release context")
    existing = subprocess.run(["gh", "api", f"repos/{repo}/releases/tags/{meta['tag']}"],
                              text=True, capture_output=True)
    if existing.returncode == 0:
        raise ValueError("Release already exists; refusing to overwrite it")
    if not re.search(r"\bHTTP 404\b", existing.stderr):
        raise ValueError("Cannot establish whether release already exists")
    subprocess.run(["gh", "release", "create", meta["tag"], "--repo", repo,
                    "--verify-tag", "--draft", "--title", "agentVIEW " + meta["tag"],
                    "--notes-file", str(folder / "RELEASE_NOTES.md"), *map(str, files)], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["version", "metadata", "prepare", "verify", "draft"])
    parser.add_argument("--directory", type=Path, default=ROOT / "dist-appimage/release")
    args = parser.parse_args()
    try:
        if args.command == "version":
            print(version(ROOT))
        elif args.command == "metadata":
            meta = metadata(ROOT, os.environ)
            output = os.environ.get("GITHUB_OUTPUT")
            if output:
                with open(output, "a") as f:
                    f.write(f"revision={meta['revision']}\ndraft_allowed={str(meta['draft_allowed']).lower()}\n")
            print(json.dumps(meta, sort_keys=True))
        elif args.command == "prepare":
            print(json.dumps(prepare(ROOT, args.directory, os.environ), sort_keys=True))
        elif args.command == "verify":
            print(json.dumps(verify(ROOT, args.directory, os.environ)[0], sort_keys=True))
        else:
            publish(ROOT, args.directory, os.environ)
    except (ValueError, OSError, KeyError, subprocess.CalledProcessError) as e:
        parser.exit(1, f"Release refused: {e}\n")


if __name__ == "__main__":
    main()
