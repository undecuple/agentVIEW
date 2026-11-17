"""Best-effort read of a gate log file (rows newest first).
A PASS row is pending unless a LATER row (rows are newest first, so a row above it) names the same
hash with a closing word that is not negated. A PASS row with no hash cannot be closed, so it is listed. The PASS row's own text never closes it."""
import re

HASH = re.compile(r"\b(?=[0-9a-f]*[0-9])(?=[0-9a-f]*[a-f])[0-9a-f]{7,40}\b")
PASS = re.compile(r"\bPASS\b")
CLOSE = re.compile(r"\b(approved|posted|merged|read back)\b", re.I)
NEG = re.compile(r"(\bnot\b|n't\b|\bno\b|\bnever\b|\bawaiting\b|\buntil\b|\bbefore\b|\bwithout\b|\bunless\b|\bif\b|\bpending\b)[^|.;]{0,30}$", re.I)


def _closes(row):
    for m in CLOSE.finditer(row):
        if not NEG.search(row[max(0, m.start() - 40):m.start()]):
            return True
    return False


def pending(text, limit=50):
    rows = [l for l in text.splitlines() if l.startswith("|")]
    # Hashes are compared by their first 7 characters (all hashes here have at least 7).
    first_close = {}  # hash prefix -> index of the newest (lowest index) closing row naming it
    for i, row in enumerate(rows):
        if _closes(row):
            for h in HASH.findall(row):
                first_close.setdefault(h[:7], i)
    out = []
    for i, row in enumerate(rows):
        if not PASS.search(row):
            continue
        hashes = {h[:7] for h in HASH.findall(row)}
        closed = any(first_close.get(h, i) < i for h in hashes)
        if not closed:
            out.append(row[:400])
            if len(out) >= limit:
                break
    return out
