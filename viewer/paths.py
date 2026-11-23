"""Read-only path policy: realpath first, then deny rules, then the allow list."""
import fnmatch
import os
import re
import stat
import tomllib


class Refused(Exception):
    pass


# Built-in deny list. The config file can only add to it, never replace or shorten it.
DEFAULT_DENY = ["work", "game", "library", "build", ".config", ".ssh", ".gnupg", ".aws", ".kube", ".netrc",
                ".npmrc", ".git-credentials", ".env", ".env.*", "credentials*", "*token*", "id_rsa*",
                "id_ed25519*", "id_ecdsa*", "*.key", "*.pem", "*.p12", "*.pfx", "*.kdbx", "*.pgp",
                "*.sprx", "*.self", "*.elf", "*.bin", "*.rap", "*.rif", "*.edat", "*.pkg", "*.iso"]


def load_config(path):
    with open(path, "rb") as fh:
        cfg = tomllib.load(fh)
    allow = []
    if not isinstance(cfg.get("allow", []), list) or not all(isinstance(x, str) for x in cfg.get("allow", [])):
        raise ValueError("allow must be a list of strings")
    if not isinstance(cfg.get("deny_names", []), list) or not all(isinstance(x, str) for x in cfg.get("deny_names", [])):
        raise ValueError("deny_names must be a list of strings")
    home = os.path.realpath(os.path.expanduser("~"))
    roots = {"/", "/etc", "/usr", "/var", "/proc", "/sys", "/dev", "/boot", "/root", "/run", "/bin", "/sbin", "/lib", "/lib64"}
    for p in cfg.get("allow", []):
        if p.startswith("~") and not p.startswith("~/"):
            raise ValueError(f"~user entries refused: {p}")
        e = os.path.expanduser(p)
        if not os.path.isabs(e):
            raise ValueError(f"relative allow entry refused: {p}")
        r = os.path.realpath(e)
        if r in roots or r == home or home.startswith(r + os.sep) or r.startswith(("/proc/", "/sys/", "/dev/")):
            raise ValueError(f"allow entry refused (system path, home or a parent of home): {p}")
        allow.append(r)
    deny = sorted({d.lower() for d in DEFAULT_DENY} | {d.lower() for d in cfg.get("deny_names", [])})
    return {"allow": allow, "deny": deny}


def _denied(real, deny):
    for part in real.split(os.sep):
        low = part.lower()
        if low and any(fnmatch.fnmatchcase(low, d) for d in deny):
            return part
    return None


def check(path, cfg):
    """Return the resolved path if it may be read, else raise Refused."""
    if "\0" in path:
        raise Refused("bad path")
    real = os.path.realpath(os.path.expanduser(path))
    hit = _denied(real, cfg["deny"])
    if hit:
        raise Refused(f"denied name: {hit}")
    for a in cfg["allow"]:
        if real == a or (os.path.isdir(a) and real.startswith(a + os.sep)):
            if not os.path.isfile(real):
                raise Refused("not a file")
            return real
    raise Refused("not in allow list")


def listing(cfg):
    """Allowed files that exist and pass the deny rules."""
    out = []
    for a in cfg["allow"]:
        cands = [a] if os.path.isfile(a) else []
        if os.path.isdir(a):
            for root, dirs, files in os.walk(a):
                dirs[:] = [d for d in dirs if not _denied(os.path.join(root, d), cfg["deny"])]
                cands += [os.path.join(root, f) for f in files]
        for c in cands:
            try:
                out.append(check(c, cfg))
            except Refused:
                pass
    return sorted(set(out))


SECRET_RE = re.compile(
    r"-----BEGIN [A-Z ]*PRIVATE KEY-----|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}"
    r"|sk-[A-Za-z0-9_-]{20,}|AKIA[0-9A-Z]{16}|xox[baprs]-[A-Za-z0-9-]{10,}"
    r"|Bearer +[A-Za-z0-9._~+/=-]{20,}"
    r"|(?i:api[_-]?key|secret|password|token)[\"' ]*[:=] *[\"'][^\"' ]{12,}[\"']", re.I)


def redact(text):
    """Best-effort: hide obvious token shapes. Not a guarantee; the deny rules are the main control."""
    return SECRET_RE.sub("[redacted]", text)


def short(path):
    home = os.path.expanduser("~")
    return "~" + path[len(home):] if path.startswith(home + os.sep) else path


def read_checked(path, cfg, cap):
    """check(), then open without following symlinks and verify it is the same regular file.
    Returns (real, text, truncated). Reads at most cap bytes."""
    real = check(path, cfg)
    st0 = os.stat(real)
    try:
        fd = os.open(real, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    except OSError:
        raise Refused("cannot open")
    with os.fdopen(fd, "rb") as fh:
        st = os.fstat(fh.fileno())
        if not stat.S_ISREG(st.st_mode) or (st.st_dev, st.st_ino) != (st0.st_dev, st0.st_ino):
            raise Refused("file changed during check")
        if st.st_nlink > 1:
            raise Refused("hard-linked file refused")
        data = fh.read(cap + 1)
    return real, data[:cap].decode("utf-8", "replace"), len(data) > cap


def read_bytes_checked(path, cfg, cap):
    """Like read_checked but returns raw bytes and refuses (never truncates) files over cap."""
    real = check(path, cfg)
    st0 = os.stat(real)
    try:
        fd = os.open(real, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    except OSError:
        raise Refused("cannot open")
    with os.fdopen(fd, "rb") as fh:
        st = os.fstat(fh.fileno())
        if not stat.S_ISREG(st.st_mode) or (st.st_dev, st.st_ino) != (st0.st_dev, st0.st_ino):
            raise Refused("file changed during check")
        if st.st_nlink > 1:
            raise Refused("hard-linked file refused")
        data = fh.read(cap + 1)
    if len(data) > cap:
        raise Refused("file too large")
    return real, data
