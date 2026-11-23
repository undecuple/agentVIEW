"""Adverse release contexts and asset tampering; never creates a real tag/release."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("release", Path(__file__).resolve().parents[1] / "scripts/release.py")
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="agentview-release-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.git("init", "-b", "main")
        for folder in ["core", "workspace", "sessions", "src-tauri"]:
            (self.root / folder).mkdir()
            (self.root / folder / "Cargo.toml").write_text('[package]\nversion = "0.1.0"\n')
        (self.root / "src-tauri/tauri.conf.json").write_text('{"version":"0.1.0"}')
        self.git("add", ".")
        self.commit()
        self.git("update-ref", "refs/remotes/origin/main", "HEAD")
        self.sha = self.git("rev-parse", "HEAD")
        self.env = {"GITHUB_EVENT_NAME": "workflow_dispatch", "GITHUB_REF": "refs/heads/main",
                    "GITHUB_SHA": self.sha, "GITHUB_REPOSITORY": "example/agentVIEW"}
        self.out = self.root.parent / (self.root.name + "-assets")
        self.out.mkdir()
        self.addCleanup(lambda: __import__('shutil').rmtree(self.out))

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.root, check=True, text=True,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout.strip()

    def commit(self):
        self.git("-c", "user.name=Release Test", "-c", "user.email=release@example.invalid",
                 "-c", "commit.gpgsign=false", "commit", "-m", "Fixture")

    def tag_context(self):
        self.git("tag", "v0.1.0")
        self.env.update(GITHUB_EVENT_NAME="push", GITHUB_REF="refs/tags/v0.1.0")

    def assets(self):
        meta = release.metadata(self.root, self.env)
        for i, name in enumerate(release.names(meta)):
            content = (f"synthetic artifact {i}\n").encode()
            (self.out / name).write_bytes(content)
            (self.out / (name + ".sha256")).write_text(hashlib.sha256(content).hexdigest() + "  " + name + "\n")
        return release.prepare(self.root, self.out, self.env)

    def test_manual_build_validates_seven_files_but_cannot_publish(self):
        meta = self.assets()
        self.assertFalse(meta["draft_allowed"])
        self.assertEqual(len(release.verify(self.root, self.out, self.env)[1]), 7)
        with patch.object(release.subprocess, "run") as external:
            with self.assertRaisesRegex(ValueError, "Only a validated"):
                release.publish(self.root, self.out, self.env)
            external.assert_not_called()

    def test_matching_tag_only_allows_a_draft(self):
        self.tag_context()
        self.assertTrue(self.assets()["draft_allowed"])
        actual = release.subprocess.run
        calls = []

        def command(args, **kwargs):
            if args[0] == "git":
                return actual(args, **kwargs)
            calls.append(args)
            return subprocess.CompletedProcess(args, 1 if args[1] == "api" else 0, "", "HTTP 404" if args[1] == "api" else "")

        with patch.object(release.subprocess, "run", side_effect=command):
            release.publish(self.root, self.out, self.env)
        create = calls[-1]
        self.assertEqual(create[:4], ["gh", "release", "create", "v0.1.0"])
        self.assertIn("--draft", create)
        self.assertIn("--verify-tag", create)
        self.assertNotIn("--latest", create)
        self.assertEqual(len([a for a in create if a.startswith(str(self.out))]), 8)  # notes arg + seven assets

    def test_bad_tag_and_non_main_commit_refused(self):
        self.git("tag", "v9.9.9")
        self.env.update(GITHUB_EVENT_NAME="push", GITHUB_REF="refs/tags/v9.9.9")
        with self.assertRaisesRegex(ValueError, "tag must match"):
            release.metadata(self.root, self.env)
        (self.root / "new.txt").write_text("not in main")
        self.git("add", ".")
        self.commit()
        self.env["GITHUB_SHA"] = self.git("rev-parse", "HEAD")
        self.tag_context()
        with self.assertRaises(subprocess.CalledProcessError):
            release.metadata(self.root, self.env)

    def test_dirty_mismatched_version_and_sha_refused(self):
        self.env["GITHUB_SHA"] = "0" * 40
        with self.assertRaises(subprocess.CalledProcessError):
            release.metadata(self.root, self.env)
        self.env["GITHUB_SHA"] = self.sha
        (self.root / "uncommitted").write_text("dirty")
        with self.assertRaisesRegex(ValueError, "clean"):
            release.metadata(self.root, self.env)
        (self.root / "uncommitted").unlink()
        (self.root / "workspace/Cargo.toml").write_text('[package]\nversion="0.2.0"\n')
        with self.assertRaisesRegex(ValueError, "versions must agree"):
            release.version(self.root)

    def test_changed_artifact_notes_extra_file_and_symlink_refused(self):
        meta = self.assets()
        target = self.out / release.names(meta)[0]
        original = target.read_bytes()
        target.write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "Checksum"):
            release.verify(self.root, self.out, self.env)
        target.write_bytes(original)
        notes = self.out / "RELEASE_NOTES.md"
        original_notes = notes.read_text()
        notes.write_text("publish this now")
        with self.assertRaisesRegex(ValueError, "notes changed"):
            release.verify(self.root, self.out, self.env)
        notes.write_text(original_notes)
        extra = self.out / "unexpected-file"
        extra.write_text("must not upload")
        with self.assertRaisesRegex(ValueError, "exactly"):
            release.verify(self.root, self.out, self.env)
        extra.unlink()
        target.unlink()
        target.symlink_to(self.root / "core/Cargo.toml")
        with self.assertRaisesRegex(ValueError, "symlinks"):
            release.verify(self.root, self.out, self.env)

    def test_provenance_and_aggregate_tampering_refused(self):
        self.assets()
        p = self.out / "build-provenance.json"
        data = p.read_text()
        changed = json.loads(data)
        changed["commit"] = "0" * 40
        p.write_text(json.dumps(changed))
        with self.assertRaisesRegex(ValueError, "provenance"):
            release.verify(self.root, self.out, self.env)
        p.write_text(data)
        (self.out / "SHA256SUMS").write_text("wrong")
        with self.assertRaisesRegex(ValueError, "Aggregate"):
            release.verify(self.root, self.out, self.env)

    def test_existing_release_or_api_error_is_not_overwritten(self):
        self.tag_context()
        self.assets()
        actual = release.subprocess.run
        for result in [subprocess.CompletedProcess([], 0, "{}", ""),
                       subprocess.CompletedProcess([], 1, "", "network unavailable")]:
            calls = []

            def command(args, **kwargs):
                if args[0] == "git":
                    return actual(args, **kwargs)
                calls.append(args)
                return result

            with patch.object(release.subprocess, "run", side_effect=command):
                with self.assertRaises(ValueError):
                    release.publish(self.root, self.out, self.env)
            self.assertEqual(len(calls), 1)


if __name__ == "__main__":
    unittest.main()
