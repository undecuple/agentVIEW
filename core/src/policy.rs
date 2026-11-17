//! Read-only path policy: canonicalise first, then the deny rules, then the allow list.
use regex::Regex;
use std::fs;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq)]
pub struct Refused(pub String);

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub allow: Vec<PathBuf>,
    pub deny: Vec<String>,
    pub gate_file: Option<PathBuf>,
}

pub fn expand(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(h) = std::env::var_os("HOME") {
            return Path::new(&h).join(rest);
        }
    }
    PathBuf::from(p)
}

/// Built-in deny list. The config file can only add to it.
pub const DEFAULT_DENY: [&str; 33] = [
    "work", "game", "library", "build", ".config", ".ssh", ".gnupg", ".aws", ".kube", ".netrc", ".npmrc",
    ".git-credentials", ".env", ".env.*", "credentials*", "*token*", "id_rsa*", "id_ed25519*", "id_ecdsa*",
    "*.key", "*.pem", "*.p12", "*.pfx", "*.kdbx", "*.pgp", "*.sprx", "*.self", "*.elf", "*.bin", "*.rap",
    "*.rif", "*.edat", "*.pkg",
];
// *.iso is added below so the array length stays obvious.
const SYSTEM_ROOTS: [&str; 14] = [
    "/", "/etc", "/usr", "/var", "/proc", "/sys", "/dev", "/boot", "/root", "/run", "/bin", "/sbin", "/lib", "/lib64",
];

fn string_list(v: &toml::Value, k: &str) -> Result<Vec<String>, String> {
    match v.get(k) {
        None => Ok(Vec::new()),
        Some(toml::Value::Array(a)) => a
            .iter()
            .map(|x| x.as_str().map(String::from).ok_or_else(|| format!("{k}: every entry must be a string")))
            .collect(),
        Some(_) => Err(format!("{k} must be a list of strings")),
    }
}

impl Config {
    pub fn from_toml(text: &str) -> Result<Config, String> {
        let v: toml::Value = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let mut allow = Vec::new();
        for p in string_list(&v, "allow")? {
            let e = expand(&p);
            if !e.is_absolute() {
                return Err(format!("relative allow entry refused: {p}"));
            }
            let real = fs::canonicalize(&e).map_err(|_| format!("allow entry does not exist: {p}"))?;
            if real.to_str().is_none() {
                return Err("non-UTF-8 allow entry refused".into());
            }
            if SYSTEM_ROOTS.iter().any(|r| real == Path::new(r)) || ["/proc", "/sys", "/dev"].iter().any(|r| real.starts_with(r)) {
                return Err(format!("allow entry is a system path: {p}"));
            }
            if let Some(h) = &home {
                let h = fs::canonicalize(h).unwrap_or_else(|_| h.clone());
                if h.starts_with(&real) {
                    return Err(format!("allow entry is your home directory or one of its parents: {p}"));
                }
            }
            allow.push(real);
        }
        let mut deny: Vec<String> = DEFAULT_DENY.iter().map(|d| d.to_string()).collect();
        deny.push("*.iso".into());
        deny.extend(string_list(&v, "deny_names")?.iter().map(|d| d.to_lowercase()));
        deny.sort();
        deny.dedup();
        let gate_file = match v.get("gate_file") {
            None => None,
            Some(toml::Value::String(s)) => Some(expand(s)),
            Some(_) => return Err("gate_file must be a string".into()),
        };
        if let Some(g) = &gate_file {
            if !g.is_absolute() {
                return Err("relative gate_file refused".into());
            }
        }
        Ok(Config { allow, deny, gate_file })
    }

    /// Load the config file only if it and its directory are owned by us and not group/other writable.
    pub fn from_file(path: &Path) -> Result<Config, String> {
        if !path.is_absolute() {
            return Err("config path must be absolute (is HOME or XDG_CONFIG_HOME unset?)".into());
        }
        let owned_and_private = |m: &fs::Metadata| m.uid() == crate::sys::euid() && m.mode() & 0o022 == 0;
        let f = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
            .open(path)
            .map_err(|e| format!("cannot open config: {e}"))?;
        let m = f.metadata().map_err(|e| e.to_string())?;
        if !m.is_file() || !owned_and_private(&m) {
            return Err("config file must be a regular file owned by you and not group or other writable".into());
        }
        let parent = path.parent().ok_or("config has no parent directory")?;
        let pm = fs::metadata(parent).map_err(|e| e.to_string())?;
        if !pm.is_dir() || !owned_and_private(&pm) {
            return Err("config directory must be owned by you and not group or other writable".into());
        }
        let mut data = Vec::new();
        f.take(1_000_001).read_to_end(&mut data).map_err(|e| e.to_string())?;
        if data.len() > 1_000_000 {
            return Err("config file is over 1 MB".into());
        }
        Config::from_toml(&String::from_utf8(data).map_err(|_| "config is not UTF-8".to_string())?)
    }
}

/// Case-insensitive glob on characters: `*`, `?`, `[abc]`, `[a-z]`, `[!abc]` (input already lower-cased).
fn glob(pat: &[char], s: &[char]) -> bool {
    match pat.first() {
        None => s.is_empty(),
        Some('*') => glob(&pat[1..], s) || (!s.is_empty() && glob(pat, &s[1..])),
        Some('?') => !s.is_empty() && glob(&pat[1..], &s[1..]),
        Some('[') => {
            let Some(end) = pat.iter().skip(2).position(|c| *c == ']').map(|i| i + 2) else {
                return !s.is_empty() && s[0] == '[' && glob(&pat[1..], &s[1..]);
            };
            let Some(c) = s.first() else { return false };
            let set = &pat[1..end];
            let (neg, set) = if matches!(set.first(), Some('!') | Some('^')) { (true, &set[1..]) } else { (false, set) };
            let mut hit = false;
            let mut i = 0;
            while i < set.len() {
                if i + 2 < set.len() && set[i + 1] == '-' {
                    hit |= set[i] <= *c && *c <= set[i + 2];
                    i += 3;
                } else {
                    hit |= set[i] == *c;
                    i += 1;
                }
            }
            hit != neg && glob(&pat[end + 1..], &s[1..])
        }
        Some(p) => s.first() == Some(p) && glob(&pat[1..], &s[1..]),
    }
}

pub fn glob_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    glob(&p, &n)
}

fn denied(real: &Path, deny: &[String]) -> Option<String> {
    for comp in real.components() {
        let part = comp.as_os_str().to_string_lossy();
        if !part.is_empty() && deny.iter().any(|d| glob_match(d, &part)) {
            return Some(part.into_owned());
        }
    }
    None
}

pub fn check(path: &str, cfg: &Config) -> Result<PathBuf, Refused> {
    if path.contains('\0') {
        return Err(Refused("bad path".into()));
    }
    check_path(&expand(path), cfg)
}

pub fn check_path(path: &Path, cfg: &Config) -> Result<PathBuf, Refused> {
    check_root_path(path, cfg).map(|(real, _)| real)
}

/// Like check, also returns the index of the allow-list entry that admitted the file.
pub fn check_root(path: &str, cfg: &Config) -> Result<(PathBuf, usize), Refused> {
    if path.contains('\0') {
        return Err(Refused("bad path".into()));
    }
    check_root_path(&expand(path), cfg)
}

pub fn check_root_path(path: &Path, cfg: &Config) -> Result<(PathBuf, usize), Refused> {
    // A path that cannot be resolved is refused; there is no fallback to the unresolved text.
    let real = fs::canonicalize(path).map_err(|_| Refused("unresolvable path".into()))?;
    if real.to_str().is_none() {
        return Err(Refused("non-UTF-8 name".into()));
    }
    if ["/proc", "/sys", "/dev"].iter().any(|p| real.starts_with(p)) {
        return Err(Refused("system path refused".into()));
    }
    if let Some(hit) = denied(&real, &cfg.deny) {
        return Err(Refused(format!("denied name: {hit}")));
    }
    for (i, a) in cfg.allow.iter().enumerate() {
        if real == *a || (a.is_dir() && real.starts_with(a)) {
            if !real.is_file() {
                return Err(Refused("not a file".into()));
            }
            return Ok((real, i));
        }
    }
    Err(Refused("not in allow list".into()))
}

pub fn listing(cfg: &Config) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for a in &cfg.allow {
        let mut cands = Vec::new();
        if a.is_file() {
            cands.push(a.clone());
        } else if a.is_dir() {
            walk(a, cfg, &mut cands);
        }
        for c in cands {
            if let Ok(p) = check_path(&c, cfg) {
                out.push(p);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn walk(dir: &Path, cfg: &Config, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            if denied(&p, &cfg.deny).is_none() {
                walk(&p, cfg, out);
            }
        } else {
            out.push(p);
        }
    }
}

fn secret_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(concat!(
            r#"(?i)-----BEGIN [A-Z ]*PRIVATE KEY-----|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}"#,
            r#"|sk-[A-Za-z0-9_-]{20,}|AKIA[0-9A-Z]{16}|xox[baprs]-[A-Za-z0-9-]{10,}"#,
            r#"|Bearer +[A-Za-z0-9._~+/=-]{20,}"#,
            r#"|(?:api[_-]?key|secret|password|token)["' ]*[:=] *["'][^"' ]{12,}["']"#
        ))
        .unwrap()
    })
}

/// Best effort: hide obvious token shapes. The deny rules are the main control.
pub fn redact(text: &str) -> String {
    secret_re().replace_all(text, "[redacted]").into_owned()
}

pub fn short(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(h) if path.starts_with(&format!("{h}/")) => format!("~{}", &path[h.len()..]),
        _ => path.to_string(),
    }
}

/// Open below the admitting allow-list entry with openat2 (no symlinks, no escape, no blocking), verify it is a
/// regular file with the same device and inode that passed check(), then read at most cap bytes.
/// Returns the data and whether more existed.
pub fn read_bytes(path: &str, cfg: &Config, cap: usize) -> Result<(PathBuf, Vec<u8>, bool), Refused> {
    let (real, idx) = check_root(path, cfg)?;
    let root = &cfg.allow[idx];
    let (base, rel): (PathBuf, PathBuf) = if root.is_dir() {
        (root.clone(), real.strip_prefix(root).map_err(|_| Refused("not in allow list".into()))?.to_path_buf())
    } else {
        (
            root.parent().ok_or_else(|| Refused("bad allow entry".into()))?.to_path_buf(),
            PathBuf::from(root.file_name().ok_or_else(|| Refused("bad allow entry".into()))?),
        )
    };
    let st0 = fs::metadata(&real).map_err(|_| Refused("cannot open".into()))?;
    let mut f = None;
    for _ in 0..3 {
        match crate::sys::open_beneath(&base, &rel) {
            Ok(x) => {
                f = Some(x);
                break;
            }
            Err(e) if e.raw_os_error() == Some(libc::EAGAIN) => continue,
            Err(e) if matches!(e.raw_os_error(), Some(libc::ENOSYS) | Some(libc::EPERM)) => {
                return Err(Refused("openat2 is not available here; refusing to read".into()))
            }
            Err(_) => return Err(Refused("cannot open".into())),
        }
    }
    let f = f.ok_or_else(|| Refused("cannot open".into()))?;
    let st = f.metadata().map_err(|_| Refused("cannot open".into()))?;
    if !st.is_file() || (st.dev(), st.ino()) != (st0.dev(), st0.ino()) {
        return Err(Refused("file changed during check".into()));
    }
    if st.nlink() > 1 {
        return Err(Refused("hard-linked file refused".into()));
    }
    let mut data = Vec::new();
    f.take(cap as u64 + 1).read_to_end(&mut data).map_err(|_| Refused("cannot read".into()))?;
    let more = data.len() > cap;
    data.truncate(cap);
    Ok((real, data, more))
}

pub fn read_text(path: &str, cfg: &Config, cap: usize) -> Result<(PathBuf, String, bool), Refused> {
    let (real, data, more) = read_bytes(path, cfg, cap)?;
    Ok((real, String::from_utf8_lossy(&data).into_owned(), more))
}
