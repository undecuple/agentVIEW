//! Follow the conversation a Claude Code session really continues.
//!
//! Inside a running CLI the person can `/resume` another conversation; Claude Code then writes a different
//! top-level transcript `<Claude root>/projects/<slug>/<uuid>.jsonl`, and the app's saved ID goes stale, so a
//! later "Resume" would reopen the old conversation. This module only READS that project folder: file names,
//! sizes and times, plus the `cwd` and `sessionId` fields of at most the first 64 KiB of a candidate file.
//! It never reads prompts or outputs, never follows a link, and never starts, stops or types into a session.
//! Codex is not handled here: its thread IDs are bound at bootstrap and stay as they are.
use crate::{codex, sql, Provider, Result, SessionInfo, SessionManager};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// At most this many folder entries are looked at per check.
const MAX_ENTRIES: usize = 5000;
/// At most this many candidate files are opened per check (only files newer than the saved one are opened).
const MAX_OPENED: usize = 16;
/// Bytes read from the start of a candidate to find its `cwd`/`sessionId` fields.
const HEAD_BYTES: u64 = 64 * 1024;
/// Records (lines) looked at in that head.
const HEAD_LINES: usize = 60;

/// One registered Claude session as the checker needs it (copied out under the manager lock, then used without it).
#[derive(Clone, Debug)]
pub struct FollowTarget {
    pub app_session_id: String,
    pub project_dir: String,
    pub saved: String,
    /// Some(start time) while the app's own process for this session is running.
    pub started: Option<SystemTime>,
    /// Another registered session uses the same project folder.
    pub shared: bool,
    /// Conversation IDs saved by other registered sessions (never adopted).
    pub taken: Vec<String>,
    /// Conversations this session was switched away from with Undo (never switched to automatically again).
    pub declined: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Conversation { pub id: String, pub modified: i64 }

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// The saved conversation is the newest one for this folder.
    Current,
    /// Running, exactly one other conversation modified since the start, folder not shared, no cap hit: switch to it.
    Follow,
    /// Running, but more than one candidate, a shared folder or a cap hit: show it, let the person choose.
    Ambiguous,
    /// Not running and a newer conversation exists: offer "Resume latest".
    Offer,
    /// The folder or the Claude root cannot be used (moved root, override in effect, unreadable).
    Unavailable,
}

#[derive(Clone, Debug, Serialize)]
pub struct Verdict {
    pub decision: Decision,
    /// Newest valid conversation newer than the saved one.
    pub newer: Option<Conversation>,
    /// All valid newer conversations, newest first (IDs the person may pick).
    pub candidates: Vec<Conversation>,
    /// A folder-entry or opened-file cap was reached: never switched automatically.
    pub capped: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct FollowEvent { pub old_id: String, pub new_id: String, pub at: String, pub how: String }

fn unix(t: SystemTime) -> i64 { t.duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) }

/// The folder Claude Code uses for a project: every character that is not an ASCII letter or digit becomes `-`
/// (the same rule the usage reader uses). Refused when the root is not absolute and canonical, when Claude's
/// project-folder override is set, or when the folder is a link or missing.
pub fn claude_folder(root: &Path, project_dir: &str) -> Result<PathBuf> {
    if std::env::var_os("CLAUDE_CODE_PROJECT_DIR_NAME").is_some() { return Err("project folder override in effect".into()); }
    if !root.is_absolute() || fs::canonicalize(root).map_err(|_| "Claude folder missing")? != root { return Err("Claude folder is not a plain path".into()); }
    let slug: String = project_dir.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    if slug.is_empty() || slug.len() > 200 { return Err("project folder name not supported".into()); }
    let folder = root.join("projects").join(slug);
    let meta = fs::symlink_metadata(&folder).map_err(|_| "no transcripts for this folder yet")?;
    if !meta.is_dir() || meta.file_type().is_symlink() { return Err("transcript folder is not a plain folder".into()); }
    Ok(folder)
}

#[derive(Deserialize)]
struct Head { cwd: Option<String>, #[serde(rename = "sessionId")] session_id: Option<String> }

/// True when the first record that carries a `cwd` names this project folder, and no record in the head carries a
/// `sessionId` other than the file name. Later records may name other folders (the session can change directory),
/// so only that first `cwd` counts. Reads at most 64 KiB; never follows a link.
fn belongs(path: &Path, id: &str, project_dir: &str) -> bool {
    let Ok(file) = fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK).open(path) else { return false };
    let Ok(meta) = file.metadata() else { return false };
    if !meta.is_file() { return false; }
    let mut head = Vec::new();
    if file.take(HEAD_BYTES).read_to_end(&mut head).is_err() { return false; }
    let mut first_cwd: Option<bool> = None;
    for line in head.split(|b| *b == b'\n').take(HEAD_LINES) {
        let Ok(h) = serde_json::from_slice::<Head>(line) else { continue };
        if let Some(s) = h.session_id { if s != id { return false; } }
        if let (None, Some(c)) = (first_cwd, h.cwd) { first_cwd = Some(c == project_dir); }
    }
    first_cwd == Some(true)
}

/// Top-level `<uuid>.jsonl` files in `folder` whose modification time is strictly after `after`, other than `saved`
/// and `taken`, whose head belongs to `project_dir`. Newest first. Creation time does not matter: `/resume` inside
/// the CLI keeps writing into the existing transcript it picked. Links, FIFOs, folders, other names and subfolders
/// are ignored. The flag is true when a cap was reached (then the list may be incomplete).
pub fn newer_conversations(folder: &Path, project_dir: &str, saved: &str, after: SystemTime, taken: &[String]) -> (Vec<Conversation>, bool) {
    let Ok(dir) = fs::read_dir(folder) else { return (vec![], false) };
    let mut seen: Vec<(SystemTime, String, PathBuf)> = Vec::new();
    let mut entries = 0usize;
    let mut capped = false;
    for entry in dir.flatten() {
        entries += 1;
        if entries > MAX_ENTRIES { capped = true; break; }
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(id) = name.strip_suffix(".jsonl") else { continue };
        if id == saved || taken.iter().any(|t| t == id) || codex::validate_uuid(id).is_err() { continue; }
        let Ok(meta) = fs::symlink_metadata(entry.path()) else { continue };
        if !meta.file_type().is_file() { continue; }
        let Ok(modified) = meta.modified() else { continue };
        if modified <= after { continue; }
        seen.push((modified, id.to_string(), entry.path()));
    }
    seen.sort_by(|a, b| b.0.cmp(&a.0));
    if seen.len() > MAX_OPENED { capped = true; }
    let list = seen.into_iter().take(MAX_OPENED)
        .filter(|(_, id, path)| belongs(path, id, project_dir))
        .map(|(t, id, _)| Conversation { id, modified: unix(t) })
        .collect();
    (list, capped)
}

/// Decide what to do for one session. Reads only the project's transcript folder; takes no lock.
pub fn check(root: Option<&Path>, t: &FollowTarget) -> Verdict {
    let none = |decision, reason: &str| Verdict { decision, newer: None, candidates: vec![], capped: false, reason: reason.into() };
    let Some(root) = root else { return none(Decision::Unavailable, "Claude folder not found") };
    let folder = match claude_folder(root, &t.project_dir) { Ok(f) => f, Err(e) => return none(Decision::Unavailable, &e) };
    let saved_path = folder.join(format!("{}.jsonl", t.saved));
    let saved_time = fs::symlink_metadata(&saved_path).ok().filter(|m| m.file_type().is_file()).and_then(|m| m.modified().ok()).unwrap_or(UNIX_EPOCH);
    let after = match t.started { Some(s) if s > saved_time => s, _ => saved_time };
    let (candidates, capped) = newer_conversations(&folder, &t.project_dir, &t.saved, after, &t.taken);
    let newer = candidates.first().cloned();
    let (decision, reason) = match (t.started.is_some(), candidates.len()) {
        (_, 0) if capped => (Decision::Unavailable, "too many files in this folder to check"),
        (_, 0) => (Decision::Current, "the saved conversation is the most recently modified one for this folder"),
        (false, _) => (Decision::Offer, "another conversation in this folder was modified more recently"),
        (true, _) if t.shared => (Decision::Ambiguous, "another registered session uses this folder"),
        (true, _) if capped => (Decision::Ambiguous, "too many files in this folder to decide automatically"),
        (true, 1) if t.declined.contains(&candidates[0].id) => (Decision::Ambiguous, "you undid the switch to this conversation"),
        (true, 1) => (Decision::Follow, "one other conversation in this folder was modified since the session started"),
        (true, _) => (Decision::Ambiguous, "several other conversations were modified in this folder"),
    };
    Verdict { decision, newer, candidates, capped, reason: reason.into() }
}

/// Fresh re-check right before an automatic switch: the folder is read again now (not a cached verdict); the session
/// must still be running and `expected` must still be the only candidate.
pub fn confirm(root: Option<&Path>, t: &FollowTarget, expected: &str) -> Result<()> {
    if t.started.is_none() { return Err("The session is no longer running; nothing was switched".into()); }
    let v = check(root, t);
    if v.decision != Decision::Follow || v.candidates.len() != 1 || v.newer.as_ref().map(|c| c.id.as_str()) != Some(expected) {
        return Err(format!("Not switched: {}", v.reason));
    }
    Ok(())
}

impl SessionManager {
    pub(crate) fn init_follow(tx: &rusqlite::Transaction<'_>) -> Result<()> {
        sql(tx.execute_batch("CREATE TABLE IF NOT EXISTS session_follows (
            app_session_id TEXT NOT NULL, old_id TEXT NOT NULL, new_id TEXT NOT NULL, at TEXT NOT NULL, how TEXT NOT NULL);"))
    }
    /// True when `native` is the conversation of this session's LATEST recorded follow (or undo). An older followed ID is refused.
    pub(crate) fn followed(&self, id: &str, native: &str) -> Result<bool> {
        let last: Option<String> = sql(self.connection.query_row("SELECT new_id FROM session_follows WHERE app_session_id=?1 ORDER BY rowid DESC LIMIT 1", params![id], |r| r.get(0)).optional())?;
        Ok(last.as_deref() == Some(native))
    }
    /// Registered Claude sessions with a saved conversation, as plain data for `check`.
    pub fn follow_targets(&mut self) -> Result<Vec<FollowTarget>> {
        let list = self.list()?;
        let mut st = sql(self.connection.prepare("SELECT app_session_id,old_id FROM session_follows WHERE how='undo' LIMIT 4096"))?;
        let undone: Vec<(String, String)> = sql(sql(st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))))?.collect())?;
        drop(st);
        let claude: Vec<&SessionInfo> = list.iter().filter(|s| s.provider == Provider::Claude && s.attempted && s.provider_session_id.is_some()).collect();
        Ok(claude.iter().map(|s| {
            let saved = s.provider_session_id.clone().unwrap_or_default();
            FollowTarget {
                app_session_id: s.id.clone(), project_dir: s.project_dir.clone(), saved: saved.clone(),
                started: self.running.get(&s.id).filter(|r| r.child.is_some()).map(|r| r.started),
                shared: list.iter().any(|o| o.id != s.id && o.project_dir == s.project_dir),
                taken: list.iter().filter(|o| o.id != s.id).filter_map(|o| o.provider_session_id.clone()).collect(),
                declined: undone.iter().filter(|(a, _)| *a == s.id).map(|(_, o)| o.clone()).collect(),
            }
        }).collect())
    }
    /// Switch a Claude session's saved conversation from `expected_old` to `new_id` and record it.
    /// Refuses when the saved ID changed meanwhile, for Codex, or for an ID that is not a UUID or is taken.
    pub fn adopt_conversation(&mut self, id: &str, expected_old: &str, new_id: &str, how: &str) -> Result<SessionInfo> {
        self.refresh()?;
        let info = self.get(id)?;
        if info.provider != Provider::Claude { return Err("Only Claude Code sessions follow conversations".into()); }
        if info.provider_session_id.as_deref() != Some(expected_old) { return Err("The saved conversation changed meanwhile; check again".into()); }
        codex::validate_uuid(new_id)?;
        if new_id == expected_old { return Ok(info); }
        if how == "auto" && !self.running.get(id).is_some_and(|r| r.child.is_some()) { return Err("The session is no longer running; nothing was switched".into()); }
        self.switch(id, expected_old, new_id, how)?;
        self.get(id)
    }
    /// One transaction: the saved ID changes in exactly one row and the follow is recorded, or nothing changes.
    pub(crate) fn switch(&mut self, id: &str, expected_old: &str, new_id: &str, how: &str) -> Result<()> {
        let tx = sql(self.connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate))?;
        let rows = tx.execute("UPDATE sessions SET provider_session_id=?1 WHERE id=?2 AND provider=?4 AND provider_session_id=?3", params![new_id, id, expected_old, "claude"])
            .map_err(|_| "That conversation is already saved for another session")?;
        if rows != 1 { drop(tx); return Err("The saved conversation changed meanwhile; nothing was switched".into()); }
        sql(tx.execute("INSERT INTO session_follows(app_session_id,old_id,new_id,at,how) VALUES (?1,?2,?3,strftime('%Y-%m-%dT%H:%M:%SZ','now'),?4)", params![id, expected_old, new_id, how]))?;
        sql(tx.commit())
    }
    /// Undo: go back to the conversation saved before the latest follow that led to `expected` (repeatable, one step
    /// at a time, down to the app's own ID). Recorded as an "undo" follow; nothing is started.
    pub fn undo_follow(&mut self, id: &str, expected: &str, back_to: &str) -> Result<SessionInfo> {
        self.refresh()?;
        let info = self.get(id)?;
        if info.provider != Provider::Claude { return Err("Only Claude Code sessions follow conversations".into()); }
        if info.provider_session_id.as_deref() != Some(expected) { return Err("The saved conversation changed meanwhile; check again".into()); }
        let previous: Option<String> = sql(self.connection.query_row("SELECT old_id FROM session_follows WHERE app_session_id=?1 AND new_id=?2 AND how!='undo' ORDER BY rowid DESC LIMIT 1", params![id, expected], |r| r.get(0)).optional())?;
        let previous = previous.ok_or("There is no earlier conversation to go back to")?;
        if previous != back_to { return Err("The earlier conversation is not the one shown; check again".into()); }
        self.switch(id, expected, &previous, "undo")?;
        self.get(id)
    }
    /// The latest recorded follow per session.
    pub fn follow_history(&mut self) -> Result<Vec<(String, FollowEvent)>> {
        let mut st = sql(self.connection.prepare("SELECT app_session_id,old_id,new_id,at,how FROM session_follows f WHERE rowid=(SELECT max(rowid) FROM session_follows g WHERE g.app_session_id=f.app_session_id) LIMIT 64"))?;
        let rows = sql(st.query_map([], |r| Ok((r.get::<_, String>(0)?, FollowEvent { old_id: r.get(1)?, new_id: r.get(2)?, at: r.get(3)?, how: r.get(4)? }))))?;
        sql(rows.collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    const A: &str = "11111111-1111-4111-8111-111111111111";
    const B: &str = "22222222-2222-4222-8222-222222222222";
    const C: &str = "33333333-3333-4333-8333-333333333333";

    struct Fx { _t: tempfile::TempDir, root: PathBuf, project: String, folder: PathBuf }
    fn fx() -> Fx {
        let t = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(t.path()).unwrap();
        let root = base.join("claude-home");
        let project = base.join("work dir/proj.x").to_string_lossy().into_owned();
        let slug: String = project.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        let folder = root.join("projects").join(slug);
        fs::create_dir_all(&folder).unwrap();
        Fx { _t: t, root, project, folder }
    }
    fn write(f: &Fx, id: &str, cwd: &str, age_s: u64) {
        let p = f.folder.join(format!("{id}.jsonl"));
        fs::write(&p, format!("{{\"type\":\"custom-title\",\"sessionId\":\"{id}\"}}\n{{\"type\":\"system\",\"cwd\":\"{cwd}\",\"sessionId\":\"{id}\"}}\n")).unwrap();
        let t = SystemTime::now() - Duration::from_secs(age_s);
        fs::File::options().write(true).open(&p).unwrap().set_modified(t).unwrap();
    }
    fn target(f: &Fx, started_ago: Option<u64>, shared: bool) -> FollowTarget {
        FollowTarget { app_session_id: A.into(), project_dir: f.project.clone(), saved: A.into(),
            started: started_ago.map(|s| SystemTime::now() - Duration::from_secs(s)), shared, taken: vec![], declined: vec![] }
    }

    #[test] fn running_with_one_newer_conversation_in_the_same_folder_follows_it() {
        let f = fx(); write(&f, A, &f.project, 300); write(&f, B, &f.project, 10);
        let v = check(Some(&f.root), &target(&f, Some(120), false));
        assert_eq!(v.decision, Decision::Follow); assert_eq!(v.newer.unwrap().id, B);
    }
    #[test] fn a_shared_folder_never_switches() {
        let f = fx(); write(&f, A, &f.project, 300); write(&f, B, &f.project, 10);
        assert_eq!(check(Some(&f.root), &target(&f, Some(120), true)).decision, Decision::Ambiguous);
    }
    #[test] fn several_newer_conversations_are_ambiguous() {
        let f = fx(); write(&f, A, &f.project, 300); write(&f, B, &f.project, 10); write(&f, C, &f.project, 20);
        let v = check(Some(&f.root), &target(&f, Some(120), false));
        assert_eq!(v.decision, Decision::Ambiguous); assert_eq!(v.newer.unwrap().id, B); assert_eq!(v.candidates.len(), 2);
    }
    #[test] fn another_cwd_or_session_id_or_no_cwd_is_not_a_candidate() {
        let f = fx(); write(&f, A, &f.project, 300); write(&f, B, "/srv/other", 10);
        assert_eq!(check(Some(&f.root), &target(&f, Some(120), false)).decision, Decision::Current);
        let p = f.folder.join(format!("{C}.jsonl"));
        fs::write(&p, format!("{{\"cwd\":\"{}\",\"sessionId\":\"{A}\"}}\n", f.project)).unwrap();
        assert_eq!(check(Some(&f.root), &target(&f, Some(120), false)).decision, Decision::Current, "sessionId must match the file name");
        fs::write(&p, "{\"type\":\"summary\"}\n").unwrap();
        assert_eq!(check(Some(&f.root), &target(&f, Some(120), false)).decision, Decision::Current, "a head without cwd is not trusted");
    }
    #[test] fn links_other_names_and_subfolders_are_ignored() {
        let f = fx(); write(&f, A, &f.project, 300);
        let real = f.root.join(format!("{B}.jsonl"));
        fs::write(&real, format!("{{\"cwd\":\"{}\",\"sessionId\":\"{B}\"}}\n", f.project)).unwrap();
        std::os::unix::fs::symlink(&real, f.folder.join(format!("{B}.jsonl"))).unwrap();
        fs::write(f.folder.join("notes.jsonl"), format!("{{\"cwd\":\"{}\"}}\n", f.project)).unwrap();
        let sub = f.folder.join(A).join("subagents"); fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join(format!("{C}.jsonl")), format!("{{\"cwd\":\"{}\",\"sessionId\":\"{C}\"}}\n", f.project)).unwrap();
        assert_eq!(check(Some(&f.root), &target(&f, Some(120), false)).decision, Decision::Current);
    }
    #[test] fn a_linked_project_folder_is_refused() {
        let f = fx(); write(&f, A, &f.project, 300); write(&f, B, &f.project, 10);
        let moved = f.root.join("elsewhere"); fs::rename(&f.folder, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, &f.folder).unwrap();
        assert_eq!(check(Some(&f.root), &target(&f, Some(120), false)).decision, Decision::Unavailable);
    }
    #[test] fn only_files_written_after_the_start_count_while_running() {
        let f = fx(); write(&f, A, &f.project, 900); write(&f, B, &f.project, 600);
        assert_eq!(check(Some(&f.root), &target(&f, Some(120), false)).decision, Decision::Current);
    }
    #[test] fn stopped_sessions_get_an_offer_for_the_newest_valid_conversation() {
        let f = fx(); write(&f, A, &f.project, 900); write(&f, B, &f.project, 600); write(&f, C, &f.project, 60);
        let v = check(Some(&f.root), &target(&f, None, false));
        assert_eq!(v.decision, Decision::Offer); assert_eq!(v.newer.unwrap().id, C);
        write(&f, A, &f.project, 5);
        assert_eq!(check(Some(&f.root), &target(&f, None, false)).decision, Decision::Current, "saved is newest again");
    }
    #[test] fn taken_conversations_are_never_offered() {
        let f = fx(); write(&f, A, &f.project, 900); write(&f, B, &f.project, 60);
        let mut t = target(&f, None, false); t.taken = vec![B.into()];
        assert_eq!(check(Some(&f.root), &t).decision, Decision::Current);
    }

    fn touch(f: &Fx, id: &str, age_s: u64) {
        fs::File::options().write(true).open(f.folder.join(format!("{id}.jsonl"))).unwrap().set_modified(SystemTime::now() - Duration::from_secs(age_s)).unwrap();
    }
    #[test] fn an_existing_older_transcript_written_again_after_the_start_is_followed() {
        // The real /resume case: the CLI keeps writing into a days-old transcript; creation time does not matter.
        let f = fx(); write(&f, A, &f.project, 300); write(&f, B, &f.project, 5 * 86400);
        assert_eq!(check(Some(&f.root), &target(&f, Some(120), false)).decision, Decision::Current);
        touch(&f, B, 3);
        let v = check(Some(&f.root), &target(&f, Some(120), false));
        assert_eq!(v.decision, Decision::Follow); assert_eq!(v.newer.unwrap().id, B);
    }
    #[test] fn only_the_first_cwd_counts_later_records_may_name_other_folders() {
        let f = fx(); write(&f, A, &f.project, 300);
        fs::write(f.folder.join(format!("{B}.jsonl")), format!("{{\"cwd\":\"{}\",\"sessionId\":\"{B}\"}}\n{{\"cwd\":\"/srv/elsewhere\",\"sessionId\":\"{B}\"}}\n", f.project)).unwrap();
        assert_eq!(check(Some(&f.root), &target(&f, Some(120), false)).decision, Decision::Follow);
        fs::write(f.folder.join(format!("{B}.jsonl")), format!("{{\"cwd\":\"/srv/elsewhere\",\"sessionId\":\"{B}\"}}\n{{\"cwd\":\"{}\",\"sessionId\":\"{B}\"}}\n", f.project)).unwrap();
        assert_eq!(check(Some(&f.root), &target(&f, Some(120), false)).decision, Decision::Current, "the first cwd names another folder");
    }
    #[test] fn a_fifo_and_a_folder_named_like_a_transcript_are_ignored() {
        let f = fx(); write(&f, A, &f.project, 300);
        assert!(std::process::Command::new("mkfifo").arg(f.folder.join(format!("{B}.jsonl"))).status().unwrap().success());
        fs::create_dir(f.folder.join(format!("{C}.jsonl"))).unwrap();
        let v = check(Some(&f.root), &target(&f, Some(120), false));
        assert_eq!(v.decision, Decision::Current); assert!(v.candidates.is_empty());
    }
    #[test] fn more_than_sixteen_candidates_never_switch_automatically() {
        let f = fx(); write(&f, A, &f.project, 300);
        for i in 0..17 { write(&f, &format!("44444444-4444-4444-8444-{i:012}"), &f.project, 5 + i); }
        let v = check(Some(&f.root), &target(&f, Some(120), false));
        assert!(v.capped); assert_eq!(v.decision, Decision::Ambiguous); assert_eq!(v.candidates.len(), 16, "still offered to the person");
        // Even a single valid candidate among many unreadable ones is not switched once the cap is hit.
        let g = fx(); write(&g, A, &g.project, 300); write(&g, B, &g.project, 2);
        for i in 0..16 { write(&g, &format!("55555555-5555-4555-8555-{i:012}"), "/srv/other", 5 + i); }
        let v = check(Some(&g.root), &target(&g, Some(120), false));
        assert!(v.capped); assert_eq!(v.decision, Decision::Ambiguous); assert_eq!(v.candidates.len(), 1);
    }
    #[test] fn a_cached_follow_is_read_again_and_refused_when_a_sibling_appears() {
        let f = fx(); write(&f, A, &f.project, 300); write(&f, B, &f.project, 10);
        let t = target(&f, Some(120), false);
        assert_eq!(check(Some(&f.root), &t).decision, Decision::Follow);
        assert!(confirm(Some(&f.root), &t, B).is_ok());
        write(&f, C, &f.project, 2);
        assert!(confirm(Some(&f.root), &t, B).is_err(), "two candidates now");
        assert!(confirm(Some(&f.root), &target(&f, None, false), B).is_err(), "not running");
    }
    #[test] fn an_undone_conversation_is_never_switched_to_automatically_again() {
        let f = fx(); write(&f, A, &f.project, 300); write(&f, B, &f.project, 10);
        let mut t = target(&f, Some(120), false); t.declined = vec![B.into()];
        let v = check(Some(&f.root), &t);
        assert_eq!(v.decision, Decision::Ambiguous); assert_eq!(v.newer.unwrap().id, B, "still offered");
    }
}
