//! Subagents a Claude Code session spawned, read from Claude Code's own files: `<claude>/projects/<slug>/<session>/subagents/`
//! holds `agent-<id>.meta.json` (type, description, depth, finished) and `agent-<id>.jsonl` (that subagent's transcript). This is an
//! observed on-disk format, not a documented one. Only the four named meta fields are read; of the transcript only its file time, and
//! for the newest few the numeric usage fields (the same reader as the Tasks card). Prompts and outputs are never read or shown.
//! No hook is needed. Symlinks are refused, sizes and counts are capped, descriptions are redacted, cut to 80 characters and path-like
//! words naming a denied folder are hidden.
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const META_CAP: u64 = 64 * 1024;
const MAX_SESSIONS: usize = 32;
const MAX_PER_SESSION: usize = 200;
/// Directory entries looked at in one subagents folder (all of them are sorted by time before the newest 200 are kept).
const MAX_FILES_LOOKED_AT: usize = 20_000;
/// Project folders searched for a session's folder (every one up to this, not only the newest).
const MAX_PROJECT_DIRS: usize = 5000;
/// Finished subagents whose usage is summed per report (newest first); running ones never take a slot. Counted into the shared
/// transcript cache size.
pub const MAX_TOKEN_READS: usize = 8;
/// A subagent transcript bigger than this is not read for tokens here, so one refresh reads at most 8 x 8 MB.
const TOKEN_FILE_CAP: u64 = 8 * 1024 * 1024;
const RUNNING_SECS: i64 = 10 * 60;
pub const NOTE: &str = "Subagents come from Claude Code's own session folders, an observed on-disk format, not a documented one; it may change, and files that no longer match are counted as skipped. Only type, a shortened redacted description, depth, finished flag, file time and usage numbers are read.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Sub {
    pub id: String,
    pub kind: String,
    pub description: String,
    /// "running" (not finished and its transcript changed in the last 10 minutes), "finished" or "stale" (not finished, quiet longer).
    pub state: String,
    pub depth: u32,
    /// Seconds since the transcript (or, without one, the meta file) last changed.
    pub age: Option<i64>,
    pub tokens: Option<super::tasks::Tokens>,
    pub tokens_why: String,
}

#[derive(Debug, Default, Serialize)]
/// `truncated`: more than 200 subagents, the newest 200 are listed. `more_files`: the folder had more than 20,000 entries, only the
/// first 20,000 in folder order were looked at. `search_truncated`: Claude's projects folder has more than 5000 folders and this
/// session's folder was not among the first 5000 looked at. `found`: the session's folder was found.
pub struct SessionSubs { pub session: String, pub subs: Vec<Sub>, pub skipped: usize, pub truncated: bool, pub more_files: bool, pub search_truncated: bool, pub found: bool }

#[derive(Debug, Default, Serialize)]
pub struct SubagentsReport { pub sessions: Vec<SessionSubs>, pub note: &'static str }

/// Only these fields are read; serde skips the rest.
#[derive(Deserialize)]
struct RawMeta {
    #[serde(rename = "agentType")] agent_type: Option<String>,
    description: Option<String>,
    #[serde(rename = "spawnDepth")] depth: Option<u32>,
    finished: Option<bool>,
}

/// A Claude session id as Claude Code writes it: a UUID.
pub fn session_id_ok(s: &str) -> bool {
    s.len() == 36 && s.bytes().enumerate().all(|(i, b)| if matches!(i, 8 | 13 | 18 | 23) { b == b'-' } else { b.is_ascii_hexdigit() })
}
fn agent_id_ok(s: &str) -> bool { !s.is_empty() && s.len() <= 40 && s.bytes().all(|b| b.is_ascii_alphanumeric()) }

fn kind_of(t: Option<String>, deny: &[String]) -> String {
    let t = t.unwrap_or_default();
    if t.is_empty() { return "subagent".into(); }
    if t.len() > 60 || !t.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':' | b'.')) { return "other".into(); }
    if agent_app_core::policy::denied_name(&t, deny) { return "[hidden]".into(); }
    agent_app_core::policy::redact(&t)
}

pub fn parse_meta(bytes: &[u8], id: &str, deny: &[String], transcript_age: Option<i64>, meta_age: Option<i64>) -> Option<Sub> {
    let m: RawMeta = serde_json::from_slice(bytes).ok()?;
    let finished = m.finished == Some(true);
    let age = transcript_age.or(meta_age);
    let state = if finished { "finished" } else if age.is_some_and(|a| a <= RUNNING_SECS) { "running" } else { "stale" };
    let description = super::tasks::label_of(&m.description.unwrap_or_default(), deny);
    Some(Sub { id: id.into(), kind: kind_of(m.agent_type, deny), description, state: state.into(), depth: m.depth.unwrap_or(1).min(99), age, tokens: None, tokens_why: String::new() })
}

fn read_meta(path: &Path) -> Option<Vec<u8>> {
    let f = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path).ok()?;
    let m = f.metadata().ok()?;
    if !m.is_file() || m.len() > META_CAP { return None; }
    let mut buf = Vec::new();
    f.take(META_CAP).read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn age_of(m: &std::fs::Metadata, now: std::time::SystemTime) -> Option<i64> {
    let t = m.modified().ok()?;
    Some(now.duration_since(t).map(|d| d.as_secs() as i64).unwrap_or(0))
}

/// The real (not symlinked) `<projects>/<slug>/<session>` folder for this session id, searching every project folder (up to 5000);
/// the bool says the search stopped at that limit without finding it.
fn session_dir(projects: &Path, session: &str) -> (Option<PathBuf>, bool) {
    let Ok(rd) = std::fs::read_dir(projects) else { return (None, false) };
    for (n, e) in rd.flatten().enumerate() {
        if n >= MAX_PROJECT_DIRS { return (None, true); }
        let Some(name) = e.file_name().to_str().map(str::to_string) else { continue };
        if !super::tasks::dir_name_ok(&name) { continue; }
        if !std::fs::symlink_metadata(e.path()).map(|m| m.is_dir()).unwrap_or(false) { continue; }
        let d = e.path().join(session);
        if std::fs::symlink_metadata(&d).map(|m| m.is_dir()).unwrap_or(false) { return (Some(d), false); }
    }
    (None, false)
}

/// Subagents of one session, newest first. Returns the transcript paths alongside so tokens can be filled for the newest few.
fn scan_session(projects: &Path, session: &str, deny: &[String], now: std::time::SystemTime) -> (SessionSubs, Vec<Option<PathBuf>>) {
    let mut out = SessionSubs { session: session.into(), ..Default::default() };
    let (dir, cut) = session_dir(projects, session);
    out.search_truncated = cut;
    let Some(dir) = dir else { return (out, vec![]) };
    out.found = true;
    let sd = dir.join("subagents");
    if !std::fs::symlink_metadata(&sd).map(|m| m.is_dir()).unwrap_or(false) { return (out, vec![]); }
    let Ok(rd) = std::fs::read_dir(&sd) else { return (out, vec![]) };
    let mut found: Vec<(std::time::SystemTime, String, PathBuf)> = Vec::new();
    for (n, e) in rd.flatten().enumerate() {
        if n >= MAX_FILES_LOOKED_AT { out.more_files = true; break; }
        let Ok(name) = e.file_name().into_string() else { continue };
        let Some(id) = name.strip_prefix("agent-").and_then(|r| r.strip_suffix(".meta.json")).filter(|i| agent_id_ok(i)) else { continue };
        let Ok(m) = std::fs::symlink_metadata(e.path()) else { continue };
        if !m.is_file() { out.skipped += 1; continue; }
        found.push((m.modified().unwrap_or(std::time::UNIX_EPOCH), id.to_string(), e.path()));
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    if found.len() > MAX_PER_SESSION { out.truncated = true; found.truncate(MAX_PER_SESSION); }
    let mut paths = Vec::new();
    for (_, id, meta_path) in found {
        let tp = sd.join(format!("agent-{id}.jsonl"));
        let tm = std::fs::symlink_metadata(&tp).ok().filter(|m| m.is_file());
        let meta_age = std::fs::symlink_metadata(&meta_path).ok().and_then(|m| age_of(&m, now));
        match read_meta(&meta_path).and_then(|b| parse_meta(&b, &id, deny, tm.as_ref().and_then(|m| age_of(m, now)), meta_age)) {
            Some(s) => { out.subs.push(s); paths.push(tm.map(|_| tp)); }
            None => out.skipped += 1,
        }
    }
    // Running first, then newest.
    let mut idx: Vec<usize> = (0..out.subs.len()).collect();
    idx.sort_by_key(|&i| (out.subs[i].state != "running", out.subs[i].age.unwrap_or(i64::MAX)));
    let subs = idx.iter().map(|&i| out.subs[i].clone()).collect();
    let paths = idx.iter().map(|&i| paths[i].clone()).collect();
    out.subs = subs;
    (out, paths)
}

pub fn report(root: &Path, sessions: &[String], deny: &[String]) -> SubagentsReport {
    let mut rep = SubagentsReport { note: NOTE, ..Default::default() };
    let projects = root.join("projects");
    let now = std::time::SystemTime::now();
    // Token slots go to finished or stale subagents only (newest first); running ones are counted once they finish.
    let mut all: Vec<(usize, usize, i64, PathBuf)> = Vec::new();
    for s in sessions.iter().filter(|s| session_id_ok(s)).take(MAX_SESSIONS) {
        let (ss, paths) = scan_session(&projects, s, deny, now);
        let si = rep.sessions.len();
        for (i, p) in paths.into_iter().enumerate() { if let Some(p) = p { if ss.subs[i].state != "running" { all.push((si, i, ss.subs[i].age.unwrap_or(i64::MAX), p)); } } }
        rep.sessions.push(ss);
    }
    for ss in rep.sessions.iter_mut() { for s in ss.subs.iter_mut() { s.tokens_why = if s.state == "running" { "running; counted when it finishes".into() } else { "not counted: only the 8 newest finished subagents' usage is read".into() }; } }
    all.sort_by_key(|a| a.2);
    for (k, (si, i, _, p)) in all.into_iter().enumerate() {
        let sub = &mut rep.sessions[si].subs[i];
        if k >= MAX_TOKEN_READS { continue; }
        if std::fs::symlink_metadata(&p).map(|m| m.len() > TOKEN_FILE_CAP).unwrap_or(true) { sub.tokens_why = "its transcript is over 8 MB, so it is not counted here".into(); continue; }
        match super::tasks::load_cached(root, &p.to_string_lossy()) {
            Err(why) => sub.tokens_why = why.into(),
            Ok(l) if l.tail_only => sub.tokens_why = "the transcript is over 32 MB, so its total is not known".into(),
            Ok(l) => { sub.tokens = Some(super::tasks::sum_all(&l)); sub.tokens_why.clear(); }
        }
    }
    for ss in rep.sessions.iter_mut() { for s in ss.subs.iter_mut() { if s.tokens.is_none() && s.tokens_why.is_empty() { s.tokens_why = "no usage numbers".into(); } } }
    rep
}

/// Subagents of the given Claude sessions (their session ids), running first, then newest. Read-only; never prompts or outputs.
#[tauri::command(async)]
pub fn subagents_report(window: tauri::WebviewWindow, sessions: Vec<String>) -> Result<SubagentsReport, String> {
    super::authorized(&window)?;
    if sessions.len() > 200 { return Err("Too many sessions".into()); }
    let root = super::claude_settings::root().ok_or("Home folder unknown")?;
    Ok(report(&root, &sessions, &agent_app_core::policy::builtin().deny))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn deny() -> Vec<String> { agent_app_core::policy::builtin().deny }
    const SID: &str = "0b0e5a5e-1c2d-4e5f-8a9b-0c1d2e3f4a5b";
    struct Home { _d: tempfile::TempDir, root: PathBuf, sub: PathBuf }
    fn home() -> Home {
        let d = tempfile::tempdir().unwrap(); let root = std::fs::canonicalize(d.path()).unwrap();
        let sub = root.join("projects").join("-srv-example-project").join(SID).join("subagents");
        std::fs::create_dir_all(&sub).unwrap();
        Home { _d: d, root, sub }
    }
    fn meta(h: &Home, id: &str, json: &str) { std::fs::write(h.sub.join(format!("agent-{id}.meta.json")), json).unwrap(); }
    fn tline(sid: &str, id: &str, i: u64, o: u64) -> String {
        format!(r#"{{"timestamp":"2026-11-20T10:00:00Z","sessionId":"{sid}","message":{{"id":"{id}","content":"never read","usage":{{"input_tokens":{i},"output_tokens":{o},"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#)
    }
    fn set_old(p: &Path) { let t = std::time::SystemTime::now() - std::time::Duration::from_secs(3600); std::fs::File::options().write(true).open(p).unwrap().set_modified(t).unwrap(); }

    #[test] fn reads_type_description_depth_state_and_tokens_without_content() {
        let h = home();
        meta(&h, "a1", r#"{"agentType":"general-purpose","description":"Review the parser\nsecond line never shown","spawnDepth":1,"finished":true,"prompt":"never read"}"#);
        std::fs::write(h.sub.join("agent-a1.jsonl"), [tline(SID, "m1", 10, 5), tline(SID, "m2", 1, 2)].join("\n") + "\n").unwrap();
        meta(&h, "b2", r#"{"agentType":"Explore","description":"Find callers","spawnDepth":2,"finished":false}"#);
        std::fs::write(h.sub.join("agent-b2.jsonl"), tline(SID, "m3", 7, 7) + "\n").unwrap();
        let r = report(&h.root, &[SID.to_string()], &deny());
        let s = &r.sessions[0].subs;
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].id.as_str(), s[0].kind.as_str(), s[0].state.as_str(), s[0].depth), ("b2", "Explore", "running", 2), "running first");
        assert_eq!(s[0].tokens, None); assert!(s[0].tokens_why.contains("running"));
        assert_eq!((s[1].description.as_str(), s[1].state.as_str()), ("Review the parser", "finished"));
        assert_eq!(s[1].tokens.map(|t| (t.input, t.output)), Some((11, 7)));
        let js = serde_json::to_string(&r).unwrap();
        assert!(!js.contains("never read") && !js.contains("second line") && !js.contains("example-project"), "{js}");
    }
    #[test] fn a_quiet_unfinished_subagent_is_stale() {
        let h = home();
        meta(&h, "c3", r#"{"agentType":"Plan","description":"x","finished":false}"#);
        let j = h.sub.join("agent-c3.jsonl"); std::fs::write(&j, "\n").unwrap(); set_old(&j);
        assert_eq!(report(&h.root, &[SID.to_string()], &deny()).sessions[0].subs[0].state, "stale");
    }
    #[test] fn descriptions_are_redacted_and_denied_paths_hidden() {
        let h = home();
        let tok = format!("sk-ant-{}", "a1b2c3d4e5".repeat(4));
        meta(&h, "d4", &format!(r#"{{"agentType":"general-purpose","description":"use {tok} and read /srv/u/.ssh/id_ed25519","finished":true}}"#));
        let d = &report(&h.root, &[SID.to_string()], &deny()).sessions[0].subs[0].description;
        assert!(!d.contains(&tok) && !d.contains(".ssh") && d.contains("[hidden]"), "{d}");
        assert!(d.chars().count() <= 80);
    }
    #[test] fn odd_types_are_not_passed_through() {
        assert_eq!(kind_of(Some("a b<script>".into()), &deny()), "other");
        assert_eq!(kind_of(None, &deny()), "subagent");
        assert_eq!(kind_of(Some(".ssh".into()), &deny()), "[hidden]");
    }
    #[test] fn symlinks_bad_names_big_and_malformed_files_are_refused() {
        let h = home();
        let outside = h.root.join("outside.json"); std::fs::write(&outside, r#"{"agentType":"x","description":"leak","finished":true}"#).unwrap();
        std::os::unix::fs::symlink(&outside, h.sub.join("agent-e5.meta.json")).unwrap();
        meta(&h, "f6", &format!(r#"{{"description":"{}"}}"#, "y".repeat(70_000)));
        meta(&h, "g7", "not json");
        meta(&h, "bad.id", r#"{"finished":true}"#);
        let r = report(&h.root, &[SID.to_string()], &deny());
        assert!(r.sessions[0].subs.is_empty());
        assert_eq!(r.sessions[0].skipped, 3);
        assert!(!serde_json::to_string(&r).unwrap().contains("leak"));
    }
    #[test] fn a_symlinked_session_or_subagents_folder_is_not_followed() {
        let h = home();
        let other = tempfile::tempdir().unwrap();
        let osub = other.path().join("subagents"); std::fs::create_dir(&osub).unwrap();
        std::fs::write(osub.join("agent-h8.meta.json"), r#"{"finished":true}"#).unwrap();
        let sid2 = "1b0e5a5e-1c2d-4e5f-8a9b-0c1d2e3f4a5b";
        std::os::unix::fs::symlink(other.path(), h.root.join("projects").join("-srv-example-project").join(sid2)).unwrap();
        assert!(report(&h.root, &[sid2.to_string()], &deny()).sessions[0].subs.is_empty());
        let sid3 = "2b0e5a5e-1c2d-4e5f-8a9b-0c1d2e3f4a5b";
        let d3 = h.root.join("projects").join("-srv-example-project").join(sid3); std::fs::create_dir(&d3).unwrap();
        std::os::unix::fs::symlink(&osub, d3.join("subagents")).unwrap();
        assert!(report(&h.root, &[sid3.to_string()], &deny()).sessions[0].subs.is_empty());
    }
    #[test] fn caps_and_session_id_checks() {
        let h = home();
        for i in 0..(MAX_PER_SESSION + 5) { meta(&h, &format!("x{i}"), r#"{"finished":true}"#); }
        let r = report(&h.root, &[SID.to_string(), "../etc".into(), "not-a-uuid".into()], &deny());
        assert_eq!(r.sessions.len(), 1, "ids that are not UUIDs are ignored");
        assert_eq!(r.sessions[0].subs.len(), MAX_PER_SESSION); assert!(r.sessions[0].truncated);
        let many: Vec<String> = (0..40).map(|i| format!("{:08x}-1c2d-4e5f-8a9b-0c1d2e3f4a5b", i)).collect();
        assert_eq!(report(&h.root, &many, &deny()).sessions.len(), MAX_SESSIONS);
        assert!(session_id_ok(SID) && !session_id_ok("0b0e5a5e_1c2d-4e5f-8a9b-0c1d2e3f4a5b"));
    }
    #[test] fn the_newest_are_kept_by_time_not_folder_order() {
        let h = home();
        for i in 0..(MAX_PER_SESSION + 20) {
            meta(&h, &format!("y{i}"), r#"{"finished":true}"#);
            let p = h.sub.join(format!("agent-y{i}.meta.json"));
            // y0 is the newest; later names are older, so folder order and time order differ.
            std::fs::File::options().write(true).open(&p).unwrap().set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(10 + i as u64)).unwrap();
        }
        let r = report(&h.root, &[SID.to_string()], &deny());
        let ids: Vec<&str> = r.sessions[0].subs.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids.len(), MAX_PER_SESSION); assert!(ids.contains(&"y0") && !ids.contains(&format!("y{}", MAX_PER_SESSION + 19).as_str()));
        assert!(r.sessions[0].truncated && !r.sessions[0].more_files && r.sessions[0].found);
    }
    #[test] fn a_session_in_an_older_project_folder_is_still_found() {
        let h = home();
        for i in 0..320 { std::fs::create_dir(h.root.join("projects").join(format!("-srv-p{i}"))).unwrap(); }
        meta(&h, "z1", r#"{"finished":true}"#);
        let old = h.root.join("projects").join("-srv-example-project");
        std::fs::File::open(&old).unwrap().set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(86400 * 30)).unwrap();
        let r = report(&h.root, &[SID.to_string()], &deny());
        assert_eq!(r.sessions[0].subs.len(), 1); assert!(r.sessions[0].found && !r.sessions[0].search_truncated);
    }
    #[test] fn running_subagents_take_no_token_slot_and_big_transcripts_are_not_read() {
        let h = home();
        for i in 0..(MAX_TOKEN_READS + 3) { meta(&h, &format!("r{i}"), r#"{"finished":false}"#); std::fs::write(h.sub.join(format!("agent-r{i}.jsonl")), tline(SID, &format!("r{i}"), 1, 1) + "\n").unwrap(); }
        meta(&h, "f1", r#"{"finished":true}"#); let f1 = h.sub.join("agent-f1.jsonl"); std::fs::write(&f1, tline(SID, "f1", 4, 6) + "\n").unwrap(); set_old(&f1);
        meta(&h, "big", r#"{"finished":true}"#); let big = h.sub.join("agent-big.jsonl");
        std::fs::File::create(&big).unwrap().set_len(TOKEN_FILE_CAP + 1).unwrap(); set_old(&big);
        let r = report(&h.root, &[SID.to_string()], &deny());
        let by = |id: &str| r.sessions[0].subs.iter().find(|s| s.id == id).unwrap().clone();
        assert_eq!(by("f1").tokens.map(|t| (t.input, t.output)), Some((4, 6)), "a finished one still gets a slot behind many running ones");
        assert!(by("big").tokens.is_none() && by("big").tokens_why.contains("8 MB"));
        assert!(by("r0").tokens.is_none() && by("r0").tokens_why.contains("running"));
    }
    #[test] fn an_unknown_session_gives_an_empty_list() {
        let h = home();
        let r = report(&h.root, &["3b0e5a5e-1c2d-4e5f-8a9b-0c1d2e3f4a5b".to_string()], &deny());
        assert!(r.sessions[0].subs.is_empty() && r.sessions[0].skipped == 0);
    }
}
