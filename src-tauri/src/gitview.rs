//! Read-only git view. Shows branch, ahead/behind, changed files, recent commits and an identity check for the
//! repositories that contain allow-listed paths. It runs only the fixed `git` commands listed in `run()`'s callers
//! below (status, log, two `config --local --get` reads), never with a user-chosen argument, and never writes
//! anything: no add, commit, fetch, pull, push, checkout or config change. Repo-local settings that could run
//! programs are switched off for every call, and output is size- and time-limited.
use agent_app_core::{policy, Config};
use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

const OUT_CAP: u64 = 1024 * 1024;
const WAIT: Duration = Duration::from_secs(5);
const MAX_FILES: usize = 300;
const MAX_COMMITS: usize = 15;

#[derive(Serialize)]
pub struct RepoEntry { path: String, name: String }
#[derive(Serialize, Default, PartialEq, Debug)]
pub struct ChangedFile { path: String, state: String }
#[derive(Serialize)]
pub struct CommitRow { id: String, author: String, date: String, subject: String, identity_ok: bool }
#[derive(Serialize)]
pub struct RepoView {
    path: String, branch: String, upstream: Option<String>, ahead: Option<u32>, behind: Option<u32>, detached: bool,
    changed: Vec<ChangedFile>, changed_total: usize, hidden_by_policy: usize, untracked_total: usize, conflicted: usize,
    commits: Vec<CommitRow>, local_name: Option<String>, local_email: Option<String>, identity_mismatch: usize, note: &'static str,
}

/// Run one fixed git command in `repo`. Returns stdout, or None on failure/timeout. The caller supplies only constant arguments.
fn git(repo: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo)
        .args(["-c", "core.fsmonitor=false", "-c", "core.pager=cat", "-c", "core.hooksPath=/dev/null", "-c", "diff.external=", "--no-optional-locks"])
        .args(args)
        .env_clear().env("PATH", std::env::var_os("PATH").unwrap_or_default()).env("LC_ALL", "C")
        .env("GIT_OPTIONAL_LOCKS", "0").env("GIT_TERMINAL_PROMPT", "0").env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_CONFIG_GLOBAL", "/dev/null")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || { let mut buf = Vec::new(); let _ = (&mut out).take(OUT_CAP).read_to_end(&mut buf); buf });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() < WAIT => std::thread::sleep(Duration::from_millis(15)),
            _ => { let _ = child.kill(); let _ = child.wait(); let _ = reader.join(); return None; }
        }
    };
    let buf = reader.join().ok()?;
    status.success().then_some(buf)
}

pub(crate) fn denied_name(path: &str, cfg: &Config) -> bool {
    path.split('/').any(|part| !part.is_empty() && cfg.deny.iter().any(|d| policy::glob_match(d, part)))
}

/// Repositories that contain an allow-listed path (looked for up to eight folders up). Each must resolve, pass the deny names and have a `.git` folder.
fn repos(cfg: &Config) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for a in &cfg.allow {
        let mut cur = std::fs::canonicalize(a).ok();
        for _ in 0..9 {
            let Some(p) = cur else { break };
            if p.join(".git").is_dir() {
                if p.to_str().is_some_and(|s| !denied_name(s, cfg)) && !out.contains(&p) { out.push(p); }
                break;
            }
            cur = p.parent().map(Path::to_path_buf);
        }
    }
    out.sort();
    out
}

#[tauri::command]
pub fn git_repos(window: tauri::WebviewWindow, cfg: tauri::State<Arc<Config>>) -> Result<Vec<RepoEntry>, String> {
    super::authorized(&window)?;
    Ok(repos(&cfg).into_iter().take(50).map(|p| { let s = p.to_string_lossy().into_owned(); RepoEntry { name: policy::short(&s), path: s } }).collect())
}

/// Parse `git status --porcelain=v2 --branch -z`.
fn parse_status(raw: &[u8]) -> (String, Option<String>, Option<u32>, Option<u32>, bool, Vec<ChangedFile>, usize, usize) {
    let text = String::from_utf8_lossy(raw);
    let (mut branch, mut upstream, mut ahead, mut behind, mut detached) = (String::new(), None, None, None, false);
    let (mut files, mut untracked, mut conflicted) = (Vec::new(), 0usize, 0usize);
    let mut it = text.split('\0');
    while let Some(rec) = it.next() {
        if let Some(h) = rec.strip_prefix("# branch.head ") { detached = h == "(detached)"; branch = h.chars().take(100).collect(); }
        else if let Some(u) = rec.strip_prefix("# branch.upstream ") { upstream = Some(u.chars().take(100).collect()); }
        else if let Some(ab) = rec.strip_prefix("# branch.ab ") {
            let mut p = ab.split(' ');
            ahead = p.next().and_then(|x| x.trim_start_matches('+').parse().ok());
            behind = p.next().and_then(|x| x.trim_start_matches('-').parse().ok());
        } else if let Some(rest) = rec.strip_prefix("1 ") {
            let f: Vec<&str> = rest.splitn(8, ' ').collect();
            if f.len() == 8 { files.push(ChangedFile { path: f[7].to_string(), state: f[0].to_string() }); }
        } else if let Some(rest) = rec.strip_prefix("2 ") {
            let f: Vec<&str> = rest.splitn(9, ' ').collect();
            let _orig = it.next(); // renamed/copied: the next record is the old path
            if f.len() == 9 { files.push(ChangedFile { path: f[8].to_string(), state: f[0].to_string() }); }
        } else if let Some(rest) = rec.strip_prefix("u ") {
            let f: Vec<&str> = rest.splitn(10, ' ').collect();
            conflicted += 1;
            if f.len() == 10 { files.push(ChangedFile { path: f[9].to_string(), state: "UU".into() }); }
        } else if let Some(p) = rec.strip_prefix("? ") { untracked += 1; files.push(ChangedFile { path: p.to_string(), state: "??".into() }); }
    }
    (branch, upstream, ahead, behind, detached, files, untracked, conflicted)
}

#[tauri::command]
pub fn git_status(window: tauri::WebviewWindow, cfg: tauri::State<Arc<Config>>, path: String) -> Result<RepoView, String> {
    super::authorized(&window)?;
    let repo = repos(&cfg).into_iter().find(|p| p.to_string_lossy() == path).ok_or("That folder is not a repository containing an allowed path")?;
    let raw = git(&repo, &["status", "--porcelain=v2", "--branch", "-z", "--untracked-files=normal"]).ok_or("git status failed or timed out")?;
    let (branch, upstream, ahead, behind, detached, mut files, untracked_total, conflicted) = parse_status(&raw);
    let before = files.len();
    files.retain(|f| !denied_name(&f.path, &cfg));
    let hidden_by_policy = before - files.len();
    let changed_total = files.len();
    files.truncate(MAX_FILES);
    for f in &mut files { f.path = policy::redact(&f.path).chars().take(200).collect(); }
    let cfg_get = |key: &str| git(&repo, &["config", "--local", "--get", key]).map(|b| String::from_utf8_lossy(&b).trim().chars().take(120).collect::<String>()).filter(|s| !s.is_empty());
    let local_name = cfg_get("user.name");
    let local_email = cfg_get("user.email");
    let n = format!("-n{MAX_COMMITS}");
    let log = git(&repo, &["log", &n, "--no-color", "--format=%h%x1f%an%x1f%ae%x1f%ce%x1f%as%x1f%s%x1e"]).unwrap_or_default();
    let mut commits = Vec::new();
    let mut identity_mismatch = 0;
    for rec in String::from_utf8_lossy(&log).split('\x1e') {
        let f: Vec<&str> = rec.trim_start_matches('\n').split('\x1f').collect();
        if f.len() != 6 { continue; }
        let ok = local_email.as_deref().map_or(true, |e| f[2] == e && f[3] == e);
        if !ok { identity_mismatch += 1; }
        commits.push(CommitRow { id: f[0].chars().take(12).collect(), author: f[1].chars().take(60).collect(), date: f[4].chars().take(10).collect(),
            subject: policy::redact(f[5]).chars().take(160).collect(), identity_ok: ok });
    }
    Ok(RepoView { path, branch, upstream, ahead, behind, detached, changed: files, changed_total, hidden_by_policy, untracked_total, conflicted,
        commits, local_name, local_email, identity_mismatch,
        note: "Read-only. Runs only git status, git log and two git config reads. Nothing is staged, committed, fetched or pushed." })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn parses_branch_ahead_behind_and_entries() {
        let raw = b"# branch.oid abc\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0\
1 .M N... 100644 100644 100644 a b src/a.rs\0\
2 R. N... 100644 100644 100644 a b R100 new.rs\0old.rs\0\
u UU N... 1 2 3 4 a b c conflict.txt\0? new file.txt\0";
        let (b, u, a, bh, d, files, untracked, conflicted) = parse_status(raw);
        assert_eq!((b.as_str(), u.as_deref(), a, bh, d, untracked, conflicted), ("main", Some("origin/main"), Some(2), Some(1), false, 1, 1));
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["src/a.rs", "new.rs", "conflict.txt", "new file.txt"]);
    }
    #[test] fn detached_head_and_no_upstream() {
        let (b, u, a, _, d, files, ..) = parse_status(b"# branch.oid abc\0# branch.head (detached)\0");
        assert_eq!((b.as_str(), u, a, d, files.len()), ("(detached)", None, None, true, 0));
    }
    #[test] fn deny_names_hide_files() {
        let cfg = Config { deny: vec![".env".into(), "*.key".into()], ..Default::default() };
        assert!(denied_name("a/.env", &cfg) && denied_name("x.key", &cfg) && !denied_name("src/main.rs", &cfg));
    }
}
