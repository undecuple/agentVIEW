//! Which files an agent read or changed. The person pastes a one-line Claude Code PostToolUse hook; it appends one small JSON line
//! per Read, Write, Edit, MultiEdit or NotebookEdit call to a file in this app's private signals folder. The app only READS that
//! file: it never edits Claude's settings and never opens the files named in it. A line is a record that Claude touched a path,
//! not permission for anything. Names the viewer policy denies are shown as "[hidden]"; the rest is redacted and shortened.
use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const TAIL_BYTES: u64 = 256 * 1024;
const MAX_LINE: usize = 4096;
const MAX_ROWS: usize = 300;
pub const TOOLS: [&str; 5] = ["Read", "Write", "Edit", "MultiEdit", "NotebookEdit"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Access { pub t: i64, pub session: String, pub project: String, pub tool: String, pub kind: String, pub path: String, pub hidden: bool }

pub fn file() -> Result<PathBuf, String> { Ok(super::hooksignal::dir()?.join("claude-files.jsonl")) }

fn kind_of(tool: &str) -> &'static str { if tool == "Read" { "read" } else { "write" } }

/// Path shown to the page: relative to the project folder when inside it, otherwise with the home folder as `~`.
fn display(path: &str, cwd: &str) -> String {
    if !cwd.is_empty() && cwd != "/" {
        if let Some(rest) = path.strip_prefix(cwd).and_then(|r| r.strip_prefix('/')) { return rest.to_string(); }
    }
    agent_app_core::policy::short(path)
}

pub fn parse_line(line: &str, deny: &[String]) -> Option<Access> {
    if line.len() > MAX_LINE { return None; }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let t = v.get("t")?.as_i64().filter(|t| *t > 0)?;
    let session = v.get("s")?.as_str().filter(|s| super::hooksignal::safe_id(s))?.to_string();
    let tool = v.get("k")?.as_str().filter(|k| TOOLS.contains(k))?.to_string();
    let raw = v.get("p")?.as_str()?;
    if raw.is_empty() || raw.len() > 600 || !raw.starts_with('/') || raw.contains('\0') || raw.chars().any(|c| c.is_control()) { return None; }
    let cwd = super::hooksignal::clean(v.get("c").and_then(|c| c.as_str()).unwrap_or(""), 300);
    let project = Path::new(&cwd).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let hidden = agent_app_core::policy::denied_name(raw, deny) || agent_app_core::policy::denied_name(&cwd, deny);
    let path = if hidden { "[hidden]".to_string() } else { agent_app_core::policy::redact(&display(raw, &cwd)) };
    Some(Access { t, session, project: if hidden { String::new() } else { project }, kind: kind_of(&tool).into(), tool, path, hidden })
}

/// The newest rows from the end of the file (at most `limit`, newest first), optionally for one session. Refuses symlinks.
pub fn read_recent(path: &Path, deny: &[String], session: Option<&str>, limit: usize) -> Vec<Access> {
    let Ok(mut f) = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(path) else { return vec![] };
    let Ok(meta) = f.metadata() else { return vec![] };
    if !meta.is_file() { return vec![]; }
    let start = meta.len().saturating_sub(TAIL_BYTES);
    if f.seek(SeekFrom::Start(start)).is_err() { return vec![]; }
    let mut buf = Vec::new();
    if (&mut f).take(TAIL_BYTES).read_to_end(&mut buf).is_err() { return vec![]; }
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<&str> = text.lines().collect();
    if start > 0 && !lines.is_empty() { lines.remove(0); } // the first line of a tail read may be cut
    if !text.ends_with('\n') && !lines.is_empty() { lines.pop(); } // so may an unfinished last line
    let mut rows: Vec<Access> = lines.into_iter().filter_map(|l| parse_line(l, deny)).filter(|a| session.is_none_or(|s| a.session == s)).collect();
    rows.reverse();
    rows.truncate(limit.clamp(1, MAX_ROWS));
    rows
}

#[derive(Serialize)]
pub struct FilesStatus { file: String, snippet: String, exists: bool, last_at: Option<i64> }

#[tauri::command]
pub fn files_access_status(window: tauri::WebviewWindow) -> Result<FilesStatus, String> {
    super::authorized(&window)?;
    let path = file()?;
    let p = path.to_string_lossy().into_owned();
    if !super::hooksignal::snippet_safe(&p) { return Err("The signals folder path has characters the snippet cannot carry safely".into()); }
    let command = format!("jq -c '{{t:(now|floor),s:.session_id,c:.cwd,k:.tool_name,p:(.tool_input.file_path // .tool_input.notebook_path // \"\")}}' >> \"{p}\"");
    let snippet = serde_json::to_string_pretty(&serde_json::json!({"hooks": {"PostToolUse": [{"matcher": TOOLS.join("|"), "hooks": [{"type": "command", "command": command}]}]}})).map_err(|_| "snippet failed")?;
    let last = read_recent(&path, &agent_app_core::policy::builtin().deny, None, 1).first().map(|a| a.t);
    Ok(FilesStatus { file: p, snippet, exists: path.is_file(), last_at: last })
}

/// Newest accesses first. `session` narrows to one Claude session id; `limit` is capped. File contents are never read.
#[tauri::command(async)]
pub fn files_access_report(window: tauri::WebviewWindow, session: Option<String>, limit: Option<usize>) -> Result<Vec<Access>, String> {
    super::authorized(&window)?;
    if let Some(s) = &session { if !super::hooksignal::safe_id(s) { return Err("Bad session id".into()); } }
    Ok(read_recent(&file()?, &agent_app_core::policy::builtin().deny, session.as_deref(), limit.unwrap_or(100)))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn deny() -> Vec<String> { agent_app_core::policy::builtin().deny }
    const OK: &str = r#"{"t":1700000000,"s":"abc-123","c":"/p/proj","k":"Read","p":"/p/proj/src/main.rs"}"#;
    #[test] fn parses_a_read_and_shows_the_path_inside_the_project_relative() {
        let a = parse_line(OK, &deny()).unwrap();
        assert_eq!((a.kind.as_str(), a.path.as_str(), a.project.as_str(), a.hidden), ("read", "src/main.rs", "proj", false));
        let w = parse_line(&OK.replace("Read", "Edit"), &deny()).unwrap(); assert_eq!(w.kind, "write");
    }
    #[test] fn rejects_bad_lines() {
        for bad in ["", "nope", r#"{"t":0,"s":"a","k":"Read","p":"/x"}"#, r#"{"t":1,"s":"bad id","k":"Read","p":"/x"}"#, r#"{"t":1,"s":"a","k":"Bash","p":"/x"}"#,
            r#"{"t":1,"s":"a","k":"Read","p":"relative/x"}"#, r#"{"t":1,"s":"a","k":"Read","p":""}"#, r#"{"t":1,"s":"a","k":"Read"}"#, r#"{"t":1,"s":"a","k":"Read","p":"/x\u0007y"}"#] {
            assert!(parse_line(bad, &deny()).is_none(), "{bad}");
        }
        assert!(parse_line(&format!(r#"{{"t":1,"s":"a","k":"Read","p":"/{}"}}"#, "y".repeat(5000)), &deny()).is_none());
    }
    #[test] fn denied_names_never_reach_the_page() {
        for p in ["/srv/u/.ssh/id_ed25519", "/p/proj/.env", "/p/proj/secrets/.git-credentials"] {
            let line = format!(r#"{{"t":5,"s":"a","c":"/p/proj","k":"Read","p":"{p}"}}"#);
            let a = parse_line(&line, &deny()).unwrap();
            assert!(a.hidden, "{p}"); assert_eq!(a.path, "[hidden]"); assert!(!serde_json::to_string(&a).unwrap().contains("ssh") && !serde_json::to_string(&a).unwrap().contains(".env"), "{p}");
        }
    }
    #[test] fn key_shaped_text_in_a_path_is_redacted() {
        let tok = format!("sk-ant-{}", "a1b2c3d4e5".repeat(4));
        let a = parse_line(&format!(r#"{{"t":5,"s":"a","c":"/p/proj","k":"Read","p":"/p/proj/{tok}.txt"}}"#), &deny()).unwrap();
        assert!(!a.path.contains(&tok), "{}", a.path);
    }
    #[test] fn reads_newest_first_filters_by_session_and_survives_a_cut_tail() {
        let d = tempfile::tempdir().unwrap(); let p = d.path().join("f.jsonl");
        let mut s = String::new();
        for i in 1..=5 { s += &format!("{{\"t\":{i},\"s\":\"{}\",\"c\":\"/p\",\"k\":\"Read\",\"p\":\"/p/f{i}\"}}\n", if i % 2 == 0 { "even" } else { "odd" }); }
        s += "{\"t\":9,\"s\":\"odd\",\"c\":\"/p\",\"k\":\"Read\",\"p\":\"/p/unfini"; // unfinished last line
        std::fs::write(&p, &s).unwrap();
        let all = read_recent(&p, &deny(), None, 100); assert_eq!(all.iter().map(|a| a.t).collect::<Vec<_>>(), vec![5, 4, 3, 2, 1]);
        assert_eq!(read_recent(&p, &deny(), Some("even"), 100).len(), 2);
        assert_eq!(read_recent(&p, &deny(), None, 2).len(), 2);
    }
    #[test] fn symlinks_are_refused() {
        let d = tempfile::tempdir().unwrap(); let real = d.path().join("r"); std::fs::write(&real, format!("{OK}\n")).unwrap();
        let link = d.path().join("l"); std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(read_recent(&link, &deny(), None, 10).is_empty());
    }
}
