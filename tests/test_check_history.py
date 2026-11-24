import os, shutil, subprocess, tempfile, unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RE = r"^[0-9]+\+undecuple@users\.noreply\.github\.com$|^noreply@github\.com$"
BOT = "49699333+dependabot[bot]@users.noreply.github.com"
OWN = "1+undecuple@users.noreply.github.com"
# Addresses built at run time so no literal e-mail address other than a noreply one appears in this file.
WEB = "noreply" + "@github.com"
SUPPORT = "support" + "@github.com"
TRAILER = "Co-authored-by: dependabot[bot] <" + BOT + ">"


class CheckHistory(unittest.TestCase):
    def setUp(self):
        self.d = Path(tempfile.mkdtemp())
        (self.d / "scripts").mkdir()
        shutil.copy(ROOT / "scripts" / "check-history.sh", self.d / "scripts")
        scan = self.d / "scripts" / "scan-files.sh"
        scan.write_text("#!/usr/bin/env bash\ncat >/dev/null\n")
        scan.chmod(0o755)
        self.git("init", "-q")

    def tearDown(self):
        shutil.rmtree(self.d, ignore_errors=True)

    def git(self, *a, env=None):
        e = dict(os.environ, GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_SYSTEM="/dev/null")
        e.update(env or {})
        return subprocess.run(["git", *a], cwd=self.d, env=e, check=True, capture_output=True, text=True)

    def commit(self, msg, author=OWN, committer=WEB):
        (self.d / "f").write_text(msg)
        self.git("add", "-A")
        self.git("-c", "user.name=x", "-c", "user.email=" + committer, "commit", "-q", "-m", msg, "--author", "x <" + author + ">")

    def run_check(self):
        return subprocess.run(["bash", "scripts/check-history.sh", RE, "HEAD", BOT], cwd=self.d, capture_output=True, text=True)

    def test_clean_history_passes(self):
        self.commit("plain change")
        self.assertEqual(self.run_check().returncode, 0)

    def test_dependabot_trailer_is_allowed_for_its_own_author(self):
        self.commit("Bump x\n\n" + TRAILER + "\nSigned-off-by: dependabot[bot] <" + SUPPORT + ">", author=BOT)
        r = self.run_check()
        self.assertEqual(r.returncode, 0, r.stdout)

    def test_same_trailer_from_another_author_still_fails(self):
        self.commit("change\n\n" + TRAILER)
        r = self.run_check()
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("attribution line", r.stdout)

    def _bot_with(self, extra):
        self.commit("Bump y\n\n" + TRAILER + "\n" + extra, author=BOT)
        r = self.run_check()
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("attribution line", r.stdout)

    def test_dependabot_commit_with_another_co_author_fails(self):
        self._bot_with("Co-Authored-By: Someone <a@b.c>")

    def test_dependabot_commit_with_session_line_fails(self):
        self._bot_with("Claude-Session: https://claude.ai/code/session_x")

    def test_dependabot_commit_with_generated_line_fails(self):
        self._bot_with("Generated with a tool")

    def test_dependabot_trailer_with_a_different_address_fails(self):
        self._bot_with("Co-authored-by: dependabot[bot] <other@x.y>")

    def test_bad_author_still_fails(self):
        self.commit("change", author="someone-else" + "@" + "elsewhere.test")
        self.assertNotEqual(self.run_check().returncode, 0)


if __name__ == "__main__":
    unittest.main()
