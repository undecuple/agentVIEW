import subprocess, sys, tempfile, unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent.parent / "scripts" / "init-repo.py"
EMAIL = "1+someone@users.noreply.github.com"

def run(*args): return subprocess.run([sys.executable, "-I", str(SCRIPT), *args], capture_output=True, text=True)

class InitRepo(unittest.TestCase):
    def test_creates_safe_repo_without_committing(self):
        with tempfile.TemporaryDirectory() as d:
            r = Path(d) / "proj"; p = run(str(r), "--email", EMAIL); self.assertEqual(p.returncode, 0, p.stderr)
            g = lambda *a: subprocess.run(["git", "-C", str(r), *a], capture_output=True, text=True).stdout.strip()
            self.assertEqual(g("config", "user.email"), EMAIL); self.assertEqual(g("config", "core.hooksPath"), ".githooks")
            self.assertEqual(g("symbolic-ref", "--short", "HEAD"), "main"); self.assertEqual(g("remote"), "")
            ig = (r / ".gitignore").read_text()
            for e in [".env", "*.key", "*.sqlite", "target/", "CLAUDE.local.md", ".claude/settings.local.json"]: self.assertIn(e, ig)
            (r / ".env").write_text("X=1\n"); (r / "ok.txt").write_text("hi\n")
            self.assertNotIn(".env", g("status", "--porcelain"))
            p2 = run(str(r), "--email", EMAIL); self.assertEqual(p2.returncode, 0); self.assertEqual((r / ".gitignore").read_text().count("managed block ---"), 1)

    def test_personal_email_refused(self):
        with tempfile.TemporaryDirectory() as d:
            p = run(d, "--email", "me@example.com"); self.assertNotEqual(p.returncode, 0)

if __name__ == "__main__":
    unittest.main()
