#!/usr/bin/env python3
"""Set up a new project directory as a safe git repository. Never commits, adds a remote or pushes.

Usage: init-repo.py [PATH] --email ID+name@users.noreply.github.com [--name NAME] [--branch main] [--no-hooks]
Does, in this order, and only what is missing:
  1. git init -b <branch> (an existing repository is kept)
  2. repository-local author and committer identity (a numbered noreply address is required, never a personal one)
  3. .gitignore entries for secrets, keys, env files, archives, databases, logs, caches, build output and agent-local files
  4. the same pre-commit hook and file scan this app uses (.githooks/pre-commit, scripts/scan-files.sh) and core.hooksPath
  5. .pr-check.toml for scripts/pre-pr-check.py with the identity pattern for this repository
Stdlib only, Python 3.11+.
"""
import argparse, re, shutil, subprocess, sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
MARK = "# --- agent-app init (managed block) ---"
IGNORE = """\
# Secrets and keys
.env
.env.*
*.key
*.pem
*.p12
*.pfx
*.kdbx
id_*
credentials*
*token*
secrets/
# Archives, installers, images of disks
*.zip
*.tar
*.tar.*
*.tgz
*.7z
*.rar
*.iso
*.AppImage
*.exe
*.msi
*.dmg
# Databases and logs
*.sqlite
*.sqlite3
*.db
*.log
# Caches and build output
.cache/
target/
dist/
build/
node_modules/
__pycache__/
*.pyc
.venv/
.cargo-home/
dist-appimage/
# Editors and OS
.DS_Store
.idea/
.vscode/
*.swp
# Agent-local files (per-person settings, session state, scratch)
CLAUDE.local.md
.claude/settings.local.json
.claude/worktrees/
.claude/projects/
.codex/
scratch/
tmp/
work/
"""
ID = re.compile(r"^[0-9]+\+[A-Za-z0-9-]+@users\.noreply\.github\.com$")

def git(repo, *a, check=True):
    r = subprocess.run(["git", "-C", str(repo), *a], capture_output=True, text=True)
    if check and r.returncode: raise SystemExit(f"git {' '.join(a)}: {r.stderr.strip()}")
    return r.stdout.strip()

def main():
    ap = argparse.ArgumentParser(); ap.add_argument("path", nargs="?", default=".")
    ap.add_argument("--email", required=True); ap.add_argument("--name", default=None)
    ap.add_argument("--branch", default="main"); ap.add_argument("--no-hooks", action="store_true")
    a = ap.parse_args()
    if not ID.match(a.email): raise SystemExit("--email must be a numbered GitHub noreply address (ID+name@users.noreply.github.com)")
    repo = Path(a.path).expanduser().resolve(); repo.mkdir(parents=True, exist_ok=True)
    done = []
    if not (repo / ".git").exists():
        subprocess.run(["git", "init", "-q", "-b", a.branch, str(repo)], check=True); done.append(f"git init -b {a.branch}")
    name = a.name or a.email.split("+", 1)[1].split("@", 1)[0]
    git(repo, "config", "user.name", name); git(repo, "config", "user.email", a.email); done.append(f"local identity {name} <{a.email}>")
    gi = repo / ".gitignore"; cur = gi.read_text() if gi.exists() else ""
    if MARK not in cur:
        gi.write_text(cur + ("\n" if cur and not cur.endswith("\n") else "") + MARK + "\n" + IGNORE + "# --- end managed block ---\n"); done.append("wrote .gitignore block")
    if not a.no_hooks:
        for src, dst in ((HERE.parent / ".githooks" / "pre-commit", repo / ".githooks" / "pre-commit"), (HERE / "scan-files.sh", repo / "scripts" / "scan-files.sh")):
            if not src.is_file(): raise SystemExit(f"missing template {src}")
            if not dst.exists():
                dst.parent.mkdir(parents=True, exist_ok=True); shutil.copy2(src, dst); dst.chmod(0o755); done.append(f"installed {dst.relative_to(repo)}")
        git(repo, "config", "core.hooksPath", ".githooks"); done.append("core.hooksPath=.githooks")
    pc = repo / ".pr-check.toml"
    if not pc.exists():
        pc.write_text('identity_regex = "^' + re.sub(r"([+.])", r"\\\\\1", a.email) + '$"\nforbidden_words = []\nai_line = ""\nmax_changed_lines = 400\n'); done.append("wrote .pr-check.toml")
    print(f"Repository ready: {repo}"); [print(f"- {d}") for d in done]
    print("Nothing was committed, no remote was added and nothing was pushed.")

if __name__ == "__main__":
    main()
