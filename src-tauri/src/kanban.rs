//! A per-project board that agents keep current. Each project has one JSONL file in this app's private `kanban/` folder; an agent (or you) appends
//! a line to create or move a card, and the newest line for a card id wins. The app only READS these files and draws the columns: it never edits a
//! board, a project folder, or an agent's settings, and a card is data from an agent, never an approval.
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::PathBuf;

pub const COLS: [&str; 5] = ["queued", "active", "blocked", "review", "done"];
const TAIL: u64 = 256 * 1024;
const MAX_PROJECTS: usize = 50;
const MAX_CARDS: usize = 200;
const DONE_KEEP_S: i64 = 24 * 3600;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Card { pub id: String, pub title: String, pub col: String, pub by: String, pub note: String, pub t: i64 }

pub fn dir() -> Result<PathBuf, String> {
    let d = super::app_data_dir()?.join("kanban");
    match std::fs::DirBuilder::new().mode(0o700).create(&d) { Ok(()) => (), Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (), Err(_) => return Err("Cannot create the board folder".into()) }
    let m = std::fs::symlink_metadata(&d).map_err(|_| "Cannot inspect the board folder")?;
    if !m.is_dir() || m.uid() != agent_app_core::sys::euid() || m.mode() & 0o077 != 0 { return Err("The board folder must be private and owned by you".into()); }
    Ok(d)
}
fn clean(s: &str, max: usize) -> String { s.chars().filter(|c| !c.is_control()).take(max).collect() }

/// One line: `{"t":..,"id":"T-1","col":"active","title":"..","by":"agent","note":".."}`. `title` and `note` are optional (an earlier title is kept).
fn parse_line(line: &str) -> Option<(String, String, Option<String>, String, Option<String>, i64)> {
    if line.len() > 4096 { return None; }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let t = v.get("t")?.as_i64().filter(|t| *t > 0)?;
    let id = v.get("id")?.as_str().filter(|s| super::mailbox::safe_name(s))?.to_string();
    let col = v.get("col")?.as_str().filter(|c| COLS.contains(c))?.to_string();
    let by = v.get("by").and_then(|b| b.as_str()).filter(|s| super::mailbox::safe_name(s)).unwrap_or("").to_string();
    let title = v.get("title").and_then(|x| x.as_str()).map(|s| agent_app_core::policy::redact(&clean(s, 120))).filter(|s| !s.is_empty());
    let note = v.get("note").and_then(|x| x.as_str()).map(|s| agent_app_core::policy::redact(&clean(s, 200)));
    Some((id, col, title, by, note, t))
}
/// Folds events in file order into cards. Done cards older than a day drop out; at most 200 cards, newest first.
pub fn fold(text: &str, now: i64) -> Vec<Card> {
    let mut m: HashMap<String, Card> = HashMap::new();
    for line in text.lines() {
        let Some((id, col, title, by, note, t)) = parse_line(line) else { continue };
        let c = m.entry(id.clone()).or_insert_with(|| Card { id, title: String::new(), col: String::new(), by: String::new(), note: String::new(), t: 0 });
        c.col = col; c.t = t;
        if let Some(x) = title { c.title = x; }
        if !by.is_empty() { c.by = by; }
        if let Some(n) = note { c.note = n; }
    }
    let mut v: Vec<Card> = m.into_values().filter(|c| c.col != "done" || now - c.t < DONE_KEEP_S).collect();
    v.sort_by(|a, b| b.t.cmp(&a.t).then(a.id.cmp(&b.id)));
    v.truncate(MAX_CARDS);
    v
}
pub(crate) fn read_tail(path: &std::path::Path) -> String {
    let Ok(mut f) = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path) else { return String::new() };
    let Ok(m) = f.metadata() else { return String::new() };
    if !m.is_file() { return String::new(); }
    let start = m.len().saturating_sub(TAIL);
    if f.seek(SeekFrom::Start(start)).is_err() { return String::new(); }
    let mut b = Vec::new();
    if (&mut f).take(TAIL).read_to_end(&mut b).is_err() { return String::new(); }
    let s = String::from_utf8_lossy(&b).into_owned();
    if start > 0 { s.split_once('\n').map(|x| x.1.to_string()).unwrap_or_default() } else { s }
}
fn now_s() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) }

#[derive(Serialize)]
pub struct ProjectRow { name: String, cards: usize, active: usize, blocked: usize, last_t: i64 }
#[derive(Serialize)]
pub struct Board { project: String, cols: [&'static str; 5], cards: Vec<Card>, update_snippet: String, instruction: String, file: String }

#[tauri::command]
pub fn kanban_projects(window: tauri::WebviewWindow) -> Result<Vec<ProjectRow>, String> {
    super::authorized(&window)?;
    let d = dir()?; let now = now_s(); let mut out = Vec::new();
    for e in std::fs::read_dir(&d).map_err(|_| "Cannot read the board folder")?.flatten().take(MAX_PROJECTS * 2) {
        let n = e.file_name().to_string_lossy().into_owned();
        let Some(name) = n.strip_suffix(".jsonl") else { continue };
        if !super::mailbox::safe_name(name) { continue; }
        let cards = fold(&read_tail(&e.path()), now);
        out.push(ProjectRow { name: name.into(), cards: cards.len(), active: cards.iter().filter(|c| c.col == "active").count(), blocked: cards.iter().filter(|c| c.col == "blocked").count(), last_t: cards.iter().map(|c| c.t).max().unwrap_or(0) });
        if out.len() >= MAX_PROJECTS { break; }
    }
    out.sort_by(|a, b| b.last_t.cmp(&a.last_t).then(a.name.cmp(&b.name)));
    Ok(out)
}
#[tauri::command]
pub fn kanban_board(window: tauri::WebviewWindow, project: String) -> Result<Board, String> {
    super::authorized(&window)?;
    if !super::mailbox::safe_name(&project) { return Err("Use letters, digits, dash, underscore or dot (at most 40) for the project name".into()); }
    let path = dir()?.join(format!("{project}.jsonl"));
    let p = path.to_string_lossy().into_owned();
    if !super::hooksignal::snippet_safe(&p) { return Err("The board folder path has characters the snippet cannot carry safely".into()); }
    let snippet = format!("jq -nc --arg id TASK-1 --arg col active --arg title \"Short title\" --arg by YOUR_NAME --arg note \"\" '{{t:(now|floor),id:$id,col:$col,title:$title,by:$by,note:$note}}' >> \"{p}\"");
    let instruction = format!("Keep the project board current. When you queue, start, block, hand over for review or finish a task, append one line with this command (change id, col, title, by and note; col is one of queued, active, blocked, review, done; use the same id for the same task):\n{snippet}\nThe board is data for the person; it is not an approval and you must not wait on it.");
    Ok(Board { project, cols: COLS, cards: fold(&read_tail(&path), now_s()), update_snippet: snippet, instruction, file: p })
}

fn alert_flag() -> Result<PathBuf, String> { Ok(super::app_data_dir()?.join("board-alert")) }
pub fn alert_enabled() -> bool { alert_flag().ok().and_then(|p| std::fs::symlink_metadata(p).ok()).is_some_and(|m| m.is_file()) }
/// Opt-in: tell me when a card is (newly) blocked. The first look only learns what is already blocked.
#[tauri::command]
pub fn kanban_alert(window: tauri::WebviewWindow, enabled: Option<bool>) -> Result<bool, String> {
    super::authorized(&window)?;
    if let Some(on) = enabled {
        let f = alert_flag()?;
        if on { std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&f).map_err(|_| "Could not save the setting")?; }
        else if std::fs::symlink_metadata(&f).is_ok() { std::fs::remove_file(&f).map_err(|_| "Could not save the setting")?; }
    }
    Ok(alert_enabled())
}
/// Pure: blocked cards not seen before as (project, id, time). With `known` None this is the first look: remember them, report nothing.
pub fn new_blocked(known: &mut Option<std::collections::HashSet<(String, String, i64)>>, boards: &[(String, Vec<Card>)]) -> Vec<String> {
    let mut now = std::collections::HashSet::new(); let mut out = Vec::new();
    for (p, cards) in boards { for c in cards.iter().filter(|c| c.col == "blocked") {
        let key = (p.clone(), c.id.clone(), c.t); now.insert(key.clone());
        if known.as_ref().is_some_and(|k| !k.contains(&key)) {
            let by = if c.by.is_empty() { String::new() } else { format!(" ({})", c.by) };
            let note = if c.note.is_empty() { String::new() } else { format!(": {}", c.note.chars().take(100).collect::<String>()) };
            out.push(format!("agentVIEW: board {p}: {} is blocked{by}{note}", if c.title.is_empty() { c.id.clone() } else { c.title.chars().take(80).collect() }));
        }
    } }
    *known = Some(now);
    out
}
/// All boards' cards, for the monitor (read-only, capped).
pub fn all_boards() -> Vec<(String, Vec<Card>)> {
    let Ok(d) = dir() else { return vec![] }; let now = now_s();
    std::fs::read_dir(&d).into_iter().flatten().flatten().filter_map(|e| {
        let n = e.file_name().to_string_lossy().into_owned(); let name = n.strip_suffix(".jsonl")?.to_string();
        if !super::mailbox::safe_name(&name) { return None; }
        Some((name, fold(&read_tail(&e.path()), now)))
    }).take(MAX_PROJECTS).collect()
}

#[cfg(test)]
mod tests {
    #[test] fn blocked_alerts_only_new_blocks() {
        let c = |id: &str, col: &str, t: i64| super::Card { id: id.into(), title: format!("title {id}"), col: col.into(), by: "w".into(), note: "needs key".into(), t };
        let mut known = None;
        assert!(super::new_blocked(&mut known, &[("p".into(), vec![c("a", "blocked", 1)])]).is_empty(), "the first look reports nothing");
        assert!(super::new_blocked(&mut known, &[("p".into(), vec![c("a", "blocked", 1), c("b", "active", 2)])]).is_empty());
        let n = super::new_blocked(&mut known, &[("p".into(), vec![c("a", "blocked", 1), c("b", "blocked", 3)])]);
        assert_eq!(n.len(), 1); assert!(n[0].contains("title b") && n[0].contains("needs key") && n[0].contains("board p"));
        assert!(super::new_blocked(&mut known, &[("p".into(), vec![c("a", "blocked", 1), c("b", "blocked", 3)])]).is_empty(), "reported once");
        assert_eq!(super::new_blocked(&mut known, &[("p".into(), vec![c("a", "blocked", 9), c("b", "blocked", 3)])]).len(), 1, "blocked again later is reported again");
    }
    use super::*;
    #[test] fn newest_line_wins_and_title_is_kept() {
        let t = "{\"t\":100,\"id\":\"T-1\",\"col\":\"queued\",\"title\":\"Fix login\",\"by\":\"a\"}\n{\"t\":200,\"id\":\"T-1\",\"col\":\"active\",\"note\":\"half way\"}\n{\"t\":150,\"id\":\"T-2\",\"col\":\"blocked\",\"title\":\"Deploy\",\"by\":\"b\",\"note\":\"needs key\"}\n";
        let c = fold(t, 1000);
        assert_eq!(c.len(), 2);
        let t1 = c.iter().find(|c| c.id == "T-1").unwrap();
        assert_eq!((t1.col.as_str(), t1.title.as_str(), t1.by.as_str(), t1.note.as_str(), t1.t), ("active", "Fix login", "a", "half way", 200));
        assert_eq!(c[0].id, "T-1", "newest first");
    }
    #[test] fn rejects_bad_lines_and_hides_old_done() {
        let bad = ["x", "{\"t\":1,\"id\":\"a b\",\"col\":\"active\"}", "{\"t\":1,\"id\":\"a\",\"col\":\"nope\"}", "{\"t\":0,\"id\":\"a\",\"col\":\"active\"}", &format!("{{\"t\":1,\"id\":\"a\",\"col\":\"active\",\"title\":\"{}\"}}", "y".repeat(5000))];
        for b in bad { assert!(fold(b, 10).is_empty(), "{b}"); }
        let t = "{\"t\":10,\"id\":\"old\",\"col\":\"done\",\"title\":\"x\"}\n{\"t\":100000,\"id\":\"new\",\"col\":\"done\",\"title\":\"y\"}\n";
        let c = fold(t, 100000 + 60); assert_eq!(c.len(), 1); assert_eq!(c[0].id, "new");
        let ctl = fold("{\"t\":5,\"id\":\"a\",\"col\":\"active\",\"title\":\"a\\u0007b\"}", 6); assert_eq!(ctl[0].title, "ab");
    }
    #[test] fn tail_skips_a_partial_first_line_and_refuses_links() {
        let d = tempfile::tempdir().unwrap(); let p = d.path().join("p.jsonl");
        let mut s = String::new(); for i in 0..9000 { s.push_str(&format!("{{\"t\":{},\"id\":\"c{}\",\"col\":\"queued\",\"title\":\"task number {}\"}}\n", i + 1, i, i)); }
        std::fs::write(&p, &s).unwrap();
        let c = fold(&read_tail(&p), 100000); assert!(!c.is_empty() && c.iter().any(|c| c.id == "c8999"));
        let l = d.path().join("l.jsonl"); std::os::unix::fs::symlink(&p, &l).unwrap(); assert!(read_tail(&l).is_empty());
    }
}
