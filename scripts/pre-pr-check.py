#!/usr/bin/env python3
"""Pre-PR cleanup check. Read-only: never pushes, posts, comments or edits anything.

Usage: pre-pr-check.py --repo PATH --base REV [--body FILE] [--title TEXT] [--config FILE]
Checks the commits in BASE..HEAD (and the draft body) for the mechanical items a gate would fail on, prints a
report and exits 1 when any item FAILs. Items that need a person's judgement are listed as "needs a human".
Config (TOML, optional, from --config or <repo>/.pr-check.toml):
  identity_regex = "^[0-9]+\\+name@users\\.noreply\\.github\\.com$"
  forbidden_words = ["internal-board", "private-project"]   # internal names that must not appear in public text
  ai_line = ""            # required AI-help line for the body; empty means none required
  max_changed_lines = 400
Stdlib only, Python 3.11+.
"""
import argparse, re, subprocess, sys, tomllib
from pathlib import Path

BAD_FILE = re.compile(r"(^|/)(\.env[^/]*|id_(rsa|ed25519)[^/]*|[^/]*\.(pem|key|p12|pfx|kdbx|sqlite3?|db|zip|7z|rar|tar|gz|tgz|iso|bin|elf|self|sprx|pkg|edat|rap|rif|log))$", re.I)
TOKEN = re.compile(r"(?i)\b(?:gh[pousr]_[A-Za-z0-9]{20,}|sk-[A-Za-z0-9_-]{20,}|AKIA[0-9A-Z]{16}|xox[baprs]-[A-Za-z0-9-]{10,}|-----BEGIN [A-Z ]*PRIVATE KEY-----)")
HOME = re.compile(r"(/home/[a-z_][a-z0-9_-]*|/Users/[A-Za-z0-9._-]+|C:\\Users\\[A-Za-z0-9._ -]+)")
EMAIL = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")
ATTRIBUTION = re.compile(r"(?im)^(co-authored-by:|claude-session:|signed-off-by:.*(claude|codex|gpt)|.*generated with \[?claude|.*claude\.ai/code|.*chatgpt\.com/share)")
FOOTER = re.compile(r"(?i)(generated with|co-authored-by|claude\.ai/code|session link)")
HEADINGS = ["why", "change", "tests", "evidence", "overlap"]

def git(repo, *args):
    r = subprocess.run(["git", "-C", repo, *args], capture_output=True, text=True)
    if r.returncode: raise SystemExit(f"git {' '.join(args)}: {r.stderr.strip()}")
    return r.stdout

def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--repo", required=True); ap.add_argument("--base", required=True)
    ap.add_argument("--body"); ap.add_argument("--title", default=""); ap.add_argument("--config")
    a = ap.parse_args()
    cfgp = Path(a.config) if a.config else Path(a.repo) / ".pr-check.toml"
    cfg = tomllib.loads(cfgp.read_text()) if cfgp.is_file() else {}
    ident = re.compile(cfg.get("identity_regex", r"^[0-9]+\+[A-Za-z0-9-]+@users\.noreply\.github\.com$"))
    words = [w.lower() for w in cfg.get("forbidden_words", [])]
    rows = []  # (status, item, detail)
    def add(status, item, detail=""): rows.append((status, item, detail))
    rng = f"{a.base}..HEAD"
    commits = git(a.repo, "rev-list", "--reverse", rng).split()
    if not commits: raise SystemExit(f"no commits in {rng}")
    # Identity and message lines
    bad_id = [c[:10] for c in commits for e in git(a.repo, "log", "-1", "--format=%ae%n%ce", c).split() if not ident.match(e)]
    add("FAIL" if bad_id else "PASS", "author and committer identity on every commit", ", ".join(sorted(set(bad_id))))
    bad_msg = [c[:10] for c in commits if ATTRIBUTION.search(git(a.repo, "log", "-1", "--format=%B", c))]
    add("FAIL" if bad_msg else "PASS", "no attribution or session lines in commit messages", ", ".join(bad_msg))
    # Scope
    add("PASS" if len(commits) == 1 else "WARN", "one focused commit", f"{len(commits)} commits")
    behind = git(a.repo, "rev-list", "--count", f"HEAD..{a.base}").strip()
    add("PASS" if behind == "0" else "WARN", "rebased on the base", f"base has {behind} newer commit(s) not in HEAD" if behind != "0" else "")
    files = git(a.repo, "diff", "--name-only", "--diff-filter=ACMR", rng).splitlines()
    bad_files = [f for f in files if BAD_FILE.search(f)]
    add("FAIL" if bad_files else "PASS", "no keys, archives, databases, logs or game-data file types", ", ".join(bad_files))
    stat = git(a.repo, "diff", "--numstat", rng).splitlines()
    changed = sum(int(p[0]) + int(p[1]) for p in (l.split("\t") for l in stat) if p[0].isdigit() and p[1].isdigit())
    cap = int(cfg.get("max_changed_lines", 400))
    add("PASS" if changed <= cap else "WARN", f"changed lines at most {cap}", f"{changed} changed; state a reason in the body if over")
    # Added lines: secrets, private paths, e-mails, internal words
    diff = git(a.repo, "diff", "-U0", rng)
    added = [l[1:] for l in diff.splitlines() if l.startswith("+") and not l.startswith("+++")]
    def hits(rx): return sorted({m.group(0)[:40] for l in added for m in [rx.search(l)] if m})
    t = hits(TOKEN); add("FAIL" if t else "PASS", "no token or private-key shapes in added lines", f"{len(t)} match(es); not shown" if t else "")
    h = hits(HOME); add("FAIL" if h else "PASS", "no home-directory paths in added lines", ", ".join(h))
    em = [e for e in hits(EMAIL) if not e.endswith("@users.noreply.github.com")]
    add("FAIL" if em else "PASS", "no e-mail addresses other than noreply in added lines", ", ".join(em))
    iw = sorted({w for w in words if any(w in l.lower() for l in added)})
    add("FAIL" if iw else "PASS", "no configured internal names in added lines", ", ".join(iw))
    heads = re.findall(r"^\+\+\+ b/.*$", diff, re.M); dup = [l for l in set(heads) if heads.count(l) > 1]
    add("FAIL" if dup else "PASS", "no duplicate new-file headers", ", ".join(sorted(set(dup))))
    r = subprocess.run(["git", "-C", a.repo, "diff", "--check", rng], capture_output=True, text=True)
    add("WARN" if r.returncode else "PASS", "no whitespace errors", r.stdout.splitlines()[0] if r.returncode else "")
    # Title and body
    if a.title:
        add("PASS" if re.match(r"^[\w./+-]+: .{4,}", a.title) else "WARN", "title is `subsystem: what changed`", a.title)
    if a.body:
        body = Path(a.body).read_text()
        pos = []
        for hname in HEADINGS:
            m = re.search(rf"(?im)^#{{1,4}}\s*\**{hname}\b|^\*\*{hname}\b", body)
            pos.append(m.start() if m else -1)
        missing = [h for h, p in zip(HEADINGS, pos) if p < 0]
        order_ok = [p for p in pos if p >= 0] == sorted(p for p in pos if p >= 0)
        add("PASS" if not missing and order_ok else "WARN", "body headings Why, Change, Tests, Evidence, Overlap in order",
            ("missing: " + ", ".join(missing)) if missing else ("out of order" if not order_ok else ""))
        add("FAIL" if FOOTER.search(body) else "PASS", "no tool or session footers in the body")
        bt = [x for x in (TOKEN.search(body), HOME.search(body)) if x]
        add("FAIL" if bt else "PASS", "no tokens or home paths in the body")
        bi = [w for w in words if w in body.lower()]
        add("FAIL" if bi else "PASS", "no configured internal names in the body", ", ".join(bi))
        ai = cfg.get("ai_line", "")
        if ai: add("PASS" if ai in body else "FAIL", "required AI-help line present", ai)
    else:
        add("WARN", "body checks skipped", "pass --body to check layout, footers and wording")
    # Report
    order = {"FAIL": 0, "WARN": 1, "PASS": 2}
    print(f"# Pre-PR check: {len(commits)} commit(s) in {rng}, head {commits[-1][:12]}\n")
    for s, i, d in sorted(rows, key=lambda r: order[r[0]]): print(f"- {s}: {i}" + (f" ({d})" if d else ""))
    print("\n## Needs a human (not checked here)\n- the test fails before the change and passes after, with a negative control\n"
          "- every test command was run locally; results stated as run or not run\n- overlap with open PRs on the same files is listed\n"
          "- wording is plain and every claim is true\n- the person who approves has signed off on this exact text and repo")
    fails = sum(1 for s, *_ in rows if s == "FAIL")
    print(f"\nResult: {'FAIL' if fails else 'PASS'} ({fails} fail, {sum(1 for s, *_ in rows if s == 'WARN')} warn). This script never pushes or posts.")
    sys.exit(1 if fails else 0)

if __name__ == "__main__":
    main()
