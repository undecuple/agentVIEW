import subprocess, sys, tempfile, unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent.parent / "scripts" / "pre-pr-check.py"
GOOD = "1+someone@users.noreply.github.com"

def git(repo, *a, env=None):
    subprocess.run(["git", "-C", str(repo), *a], check=True, capture_output=True, env=env)

def commit(repo, msg, email=GOOD):
    env = {"GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": email, "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": email, "PATH": "/usr/bin:/bin:/run/current-system/sw/bin"}
    git(repo, "add", "-A"); git(repo, "commit", "-q", "-m", msg, env=env)

class PrePrCheck(unittest.TestCase):
    def repo(self):
        d = tempfile.TemporaryDirectory(); self.addCleanup(d.cleanup); r = Path(d.name)
        git(r, "init", "-q", "-b", "main"); (r / "a.txt").write_text("base\n"); commit(r, "base")
        git(r, "checkout", "-q", "-b", "work"); return r

    def run_check(self, r, body=None, title="core: fix the thing"):
        args = [sys.executable, "-I", str(SCRIPT), "--repo", str(r), "--base", "main", "--title", title]
        if body: args += ["--body", str(body)]
        p = subprocess.run(args, capture_output=True, text=True); return p.returncode, p.stdout

    def test_clean_change_passes(self):
        r = self.repo(); (r / "b.txt").write_text("ok\n"); commit(r, "core: fix the thing")
        body = r.parent / "body.md"; body.write_text("## Why\nx\n## Change\nx\n## Tests\nx\n## Evidence\nx\n## Overlap\nnone\n")
        code, out = self.run_check(r, body); self.assertEqual(code, 0, out); self.assertIn("Result: PASS", out)

    def test_problems_fail(self):
        r = self.repo()
        (r / "n.txt").write_text("see /home/alice/notes and bob@example.com\n"); (r / ".env").write_text("X=1\n")
        commit(r, "core: x\n\nCo-Authored-By: Someone <a@b.c>", email="me@example.com")
        body = r.parent / "body.md"; body.write_text("## Why\nx\n\nGenerated with a tool\n")
        code, out = self.run_check(r, body); self.assertEqual(code, 1)
        for text in ["FAIL: author and committer identity", "FAIL: no attribution", "FAIL: no keys", "FAIL: no home-directory", "FAIL: no e-mail", "FAIL: no tool or session footers", "WARN: body headings"]:
            self.assertIn(text, out)

    def test_token_shape_fails_without_printing_it(self):
        r = self.repo(); (r / "t.txt").write_text("k=" + "ghp_" + "a" * 30 + "\n"); commit(r, "core: y")
        code, out = self.run_check(r); self.assertEqual(code, 1); self.assertNotIn("ghp_", out)

if __name__ == "__main__":
    unittest.main()
