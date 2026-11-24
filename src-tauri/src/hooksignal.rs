//! Signals from Claude Code's own Notification hook. The person pastes a one-line hook into their Claude settings; the hook
//! appends a small JSON line to a file in this app's private folder. The app only READS that file: it never edits Claude's
//! settings, never answers a prompt and never grants anything. A line is data ("Claude says it is waiting"), not an approval.
use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::PathBuf;

const READ_CAP: u64 = 256 * 1024;
const MAX_PER_READ: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Signal { pub t: i64, pub session: String, pub cwd: String, pub kind: String, pub message: String }

/// The app-owned signals folder (mode 0700), created on demand.
pub fn dir() -> Result<PathBuf, String> {
    let d = super::app_data_dir()?.join("signals");
    match std::fs::DirBuilder::new().mode(0o700).create(&d) { Ok(()) => (), Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (), Err(_) => return Err("Cannot create the signals folder".into()) }
    let m = std::fs::symlink_metadata(&d).map_err(|_| "Cannot inspect the signals folder")?;
    if !m.is_dir() || m.uid() != agent_app_core::sys::euid() || m.mode() & 0o077 != 0 { return Err("The signals folder must be private and owned by you".into()); }
    Ok(d)
}
pub fn file() -> Result<PathBuf, String> { Ok(dir()?.join("claude-notify.jsonl")) }

fn safe_id(s: &str) -> bool { !s.is_empty() && s.len() <= 80 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') }
fn clean(s: &str, max: usize) -> String { s.chars().filter(|c| !c.is_control()).take(max).collect() }

pub fn parse_line(line: &str) -> Option<Signal> {
    if line.len() > 4096 { return None; }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let t = v.get("t")?.as_i64().filter(|t| *t > 0)?;
    let session = v.get("s")?.as_str().filter(|s| safe_id(s))?.to_string();
    let cwd = clean(v.get("c").and_then(|c| c.as_str()).unwrap_or(""), 300);
    let kind = v.get("k").and_then(|k| k.as_str()).unwrap_or("").to_string();
    if kind.len() > 40 || !kind.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') { return None; }
    let message = agent_app_core::policy::redact(&clean(v.get("m").and_then(|m| m.as_str()).unwrap_or(""), 300));
    Some(Signal { t, session, cwd, kind, message })
}

/// Reads complete new lines after `offset` and advances it. A file that shrank is read again from the start. Refuses symlinks.
pub fn read_new(path: &std::path::Path, offset: &mut u64) -> Vec<Signal> {
    let Ok(mut f) = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(path) else { return vec![] };
    let Ok(meta) = f.metadata() else { return vec![] };
    if !meta.is_file() { return vec![]; }
    if meta.len() < *offset { *offset = 0; }
    if meta.len() == *offset { return vec![]; }
    if f.seek(SeekFrom::Start(*offset)).is_err() { return vec![]; }
    let mut buf = Vec::new();
    if (&mut f).take(READ_CAP).read_to_end(&mut buf).is_err() { return vec![]; }
    let Some(end) = buf.iter().rposition(|b| *b == b'\n') else { if buf.len() as u64 >= READ_CAP { *offset += buf.len() as u64; } return vec![] };
    *offset += end as u64 + 1;
    String::from_utf8_lossy(&buf[..end]).lines().filter_map(parse_line).take(MAX_PER_READ).collect()
}

pub fn file_len(path: &std::path::Path) -> u64 { std::fs::symlink_metadata(path).map(|m| if m.is_file() { m.len() } else { 0 }).unwrap_or(0) }

/// The last valid signal in the final 64 KB of the file, for the status line.
pub fn last_signal(path: &std::path::Path) -> Option<Signal> {
    let len = file_len(path);
    let mut off = len.saturating_sub(64 * 1024);
    let mut all = Vec::new();
    loop { let got = read_new(path, &mut off); if got.is_empty() { break; } all.extend(got); if all.len() > 500 { break; } }
    all.pop()
}

/// What the page shows about a signal: the kind decides the wording; the Claude message text is shown redacted and shortened.
pub fn waiting_text(kind: &str) -> Option<&'static str> {
    match kind {
        "permission_prompt" | "elicitation_dialog" | "elicitation_url_dialog" | "agent_needs_input" => Some("is waiting for your decision"),
        "idle_prompt" => Some("is waiting for your input"),
        _ => None,
    }
}

#[derive(Serialize)]
pub struct HookStatus { file: String, snippet: String, exists: bool, last_at: Option<i64>, last_kind: Option<String> }

#[tauri::command]
pub fn claude_hook_status(window: tauri::WebviewWindow) -> Result<HookStatus, String> {
    super::authorized(&window)?;
    let path = file()?;
    let p = path.to_string_lossy().into_owned();
    let command = format!("jq -c '{{t:(now|floor),s:.session_id,c:.cwd,k:(.notification_type // \"\"),m:(.message // \"\")}}' >> \"{p}\"");
    let snippet = serde_json::to_string_pretty(&serde_json::json!({"hooks": {"Notification": [{"hooks": [{"type": "command", "command": command}]}]}})).map_err(|_| "snippet failed")?;
    let last = last_signal(&path);
    Ok(HookStatus { file: p, snippet, exists: path.is_file(), last_at: last.as_ref().map(|s| s.t), last_kind: last.map(|s| s.kind) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test] fn parses_and_rejects_lines() {
        let ok = parse_line(r#"{"t":1700000000,"s":"abc-123","c":"/p/proj","k":"permission_prompt","m":"Claude needs your permission to use Bash"}"#).unwrap();
        assert_eq!((ok.session.as_str(), ok.kind.as_str()), ("abc-123", "permission_prompt"));
        for bad in ["", "not json", r#"{"t":0,"s":"a","k":"x"}"#, r#"{"t":1,"s":"bad id","k":"x"}"#, r#"{"t":1,"s":"a","k":"Bad Kind"}"#, r#"{"s":"a","k":"x"}"#] { assert!(parse_line(bad).is_none(), "{bad}"); }
        let long = format!(r#"{{"t":1,"s":"a","k":"x","m":"{}"}}"#, "y".repeat(5000)); assert!(parse_line(&long).is_none());
        let ctl = parse_line("{\"t\":1,\"s\":\"a\",\"k\":\"x\",\"m\":\"a\\u0007b\"}").unwrap(); assert_eq!(ctl.message, "ab");
    }
    #[test] fn reads_only_new_complete_lines_and_survives_truncation() {
        let d = tempfile::tempdir().unwrap(); let p = d.path().join("s.jsonl");
        std::fs::write(&p, "{\"t\":1,\"s\":\"a\",\"k\":\"idle_prompt\"}\n").unwrap();
        let mut off = file_len(&p);
        assert!(read_new(&p, &mut off).is_empty());
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        write!(f, "{{\"t\":2,\"s\":\"b\",\"k\":\"idle_prompt\"}}\n{{\"t\":3,\"s\":\"c\"").unwrap(); // second line incomplete
        let got = read_new(&p, &mut off); assert_eq!(got.len(), 1); assert_eq!(got[0].session, "b");
        write!(f, ",\"k\":\"idle_prompt\"}}\n").unwrap();
        let got = read_new(&p, &mut off); assert_eq!(got.len(), 1); assert_eq!(got[0].session, "c");
        std::fs::write(&p, "{\"t\":9,\"s\":\"z\",\"k\":\"idle_prompt\"}\n").unwrap(); // truncated and rewritten
        assert_eq!(read_new(&p, &mut off)[0].session, "z");
        assert_eq!(last_signal(&p).unwrap().session, "z");
    }
    #[test] fn symlinks_are_refused() {
        let d = tempfile::tempdir().unwrap(); let real = d.path().join("r"); std::fs::write(&real, "{\"t\":1,\"s\":\"a\",\"k\":\"idle_prompt\"}\n").unwrap();
        let link = d.path().join("l"); std::os::unix::fs::symlink(&real, &link).unwrap();
        let mut off = 0; assert!(read_new(&link, &mut off).is_empty());
    }
    #[test] fn wording() { assert!(waiting_text("permission_prompt").unwrap().contains("decision")); assert!(waiting_text("idle_prompt").unwrap().contains("input")); assert!(waiting_text("auth_success").is_none()); }
}
