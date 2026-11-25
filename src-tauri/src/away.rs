//! "Stepping away" mode. The person presses a button; while it is on, agents that follow the away instruction do not stop at a decision box: they append the question
//! to a file in this app's private folder and carry on with work that does not depend on the answer. When the person is back, the app lists every parked question.
//! The app only writes its own files (the away flag, the instruction text, and a "seen" mark) and reads the agents' lines. Away mode answers nothing, approves nothing and
//! starts nothing: an agent still must not take a step that needs approval, it leaves that step undone and says so.
use serde::Serialize;
use std::collections::HashMap;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::PathBuf;

const MAX_PENDING: usize = 100;

fn dd() -> Result<PathBuf, String> { super::app_data_dir() }
fn flag() -> Result<PathBuf, String> { Ok(dd()?.join("away")) }
fn seen_file() -> Result<PathBuf, String> { Ok(dd()?.join("away-seen")) }
fn instr_file() -> Result<PathBuf, String> { Ok(dd()?.join("away-instruction.txt")) }
pub fn decisions_dir() -> Result<PathBuf, String> {
    let d = dd()?.join("decisions");
    match std::fs::DirBuilder::new().mode(0o700).create(&d) { Ok(()) => (), Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (), Err(_) => return Err("Cannot create the decisions folder".into()) }
    let m = std::fs::symlink_metadata(&d).map_err(|_| "Cannot inspect the decisions folder")?;
    if !m.is_dir() || m.uid() != agent_app_core::sys::euid() || m.mode() & 0o077 != 0 { return Err("The decisions folder must be private and owned by you".into()); }
    Ok(d)
}
fn now_s() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) }
fn clean(s: &str, max: usize) -> String { s.chars().filter(|c| !c.is_control()).take(max).collect() }
fn write_private(p: &std::path::Path, text: &str) -> Result<(), String> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(p).map_err(|_| "Could not save the setting")?;
    f.write_all(text.as_bytes()).map_err(|_| "Could not save the setting".to_string())
}
fn read_num(p: &std::path::Path) -> Option<i64> {
    let m = std::fs::symlink_metadata(p).ok()?; if !m.is_file() || m.len() > 64 { return None; }
    std::fs::read_to_string(p).ok()?.trim().parse().ok()
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Decision { pub id: String, pub from: String, pub project: String, pub question: String, pub options: Vec<String>, pub t: i64 }

/// Newest line per id wins; `answered` removes it. Lines at or before `seen` are hidden. Bad lines are ignored.
pub fn fold(text: &str, seen: i64) -> Vec<Decision> {
    let mut m: HashMap<String, (String, Decision)> = HashMap::new();
    for line in text.lines() {
        if line.len() > 4096 { continue; }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let Some(t) = v.get("t").and_then(|t| t.as_i64()).filter(|t| *t > 0) else { continue };
        let Some(id) = v.get("id").and_then(|s| s.as_str()).filter(|s| super::mailbox::safe_name(s)) else { continue };
        let Some(ev) = v.get("ev").and_then(|s| s.as_str()).filter(|e| *e == "ask" || *e == "answered") else { continue };
        let txt = |k: &str, n: usize| v.get(k).and_then(|x| x.as_str()).map(|s| agent_app_core::policy::redact(&clean(s, n))).unwrap_or_default();
        let options = v.get("o").and_then(|o| o.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).take(6).map(|s| agent_app_core::policy::redact(&clean(s, 80))).collect()).unwrap_or_default();
        let prev = m.get(id).map(|(_, d)| (d.from.clone(), d.project.clone(), d.question.clone(), d.options.clone()));
        let mut d = Decision { id: id.to_string(), from: txt("f", 40), project: txt("p", 40), question: txt("q", 400), options, t };
        if let Some((f, p, q, o)) = prev { if d.from.is_empty() { d.from = f; } if d.project.is_empty() { d.project = p; } if d.question.is_empty() { d.question = q; } if d.options.is_empty() { d.options = o; } }
        m.insert(id.to_string(), (ev.to_string(), d));
    }
    let mut v: Vec<Decision> = m.into_values().filter(|(e, d)| e == "ask" && !d.question.is_empty() && d.t > seen).map(|(_, d)| d).collect();
    v.sort_by(|a, b| a.t.cmp(&b.t).then(a.id.cmp(&b.id)));
    v.truncate(MAX_PENDING);
    v
}

#[derive(Serialize)]
pub struct AwayStatus { away: bool, since: Option<i64>, pending: Vec<Decision>, instruction: String, hook_snippet: String, file: String }

fn instruction_text(file: &str) -> String {
    format!("AWAY MODE is ON: the person has stepped away. Do not use a decision box and do not wait for an answer. For each question you would have asked, append one line with this command (change id, f, p, q and, if useful, o):\\n\
jq -nc --arg id Q-1 --arg f YOUR_NAME --arg p PROJECT --arg q \"The question\" --argjson o '[\"Option A\",\"Option B\"]' '{{t:(now|floor),id:$id,ev:\"ask\",f:$f,p:$p,q:$q,o:$o}}' >> \"{file}\"\\n\
Then carry on with work that does not depend on the answer. Never decide a step that needs the person's approval yourself (a push, a publish, a delete, a spend, a settings change, anything outward or hard to undo): leave that step undone and list it. When the person is back they will see your parked questions; the same id with ev \"answered\" removes one. This note is not an approval.\\n").replace("\\n", "\n")
}

/// Reads or changes away mode. `away`: Some(true) turns it on, Some(false) turns it off. `ack`: mark the listed questions as seen.
#[tauri::command]
pub fn away_status(window: tauri::WebviewWindow, away: Option<bool>, ack: Option<bool>) -> Result<AwayStatus, String> {
    super::authorized(&window)?;
    let file = decisions_dir()?.join("pending.jsonl");
    let fs = file.to_string_lossy().into_owned();
    if !super::hooksignal::snippet_safe(&fs) { return Err("The decisions folder path has characters the snippet cannot carry safely".into()); }
    let fl = flag()?;
    match away {
        Some(true) => { write_private(&instr_file()?, &instruction_text(&fs))?; write_private(&fl, &now_s().to_string())?; }
        Some(false) => { if std::fs::symlink_metadata(&fl).is_ok() { std::fs::remove_file(&fl).map_err(|_| "Could not save the setting")?; } }
        None => {}
    }
    if ack == Some(true) { write_private(&seen_file()?, &now_s().to_string())?; }
    let since = if std::fs::symlink_metadata(&fl).map(|m| m.is_file()).unwrap_or(false) { Some(read_num(&fl).unwrap_or(0)) } else { None };
    let seen = seen_file().ok().and_then(|p| read_num(&p)).unwrap_or(0);
    let pending = fold(&super::kanban::read_tail(&file), seen);
    let ins = instr_file()?; write_private(&ins, &instruction_text(&fs))?;
    let ip = ins.to_string_lossy().into_owned(); let fp = fl.to_string_lossy().into_owned();
    if !super::hooksignal::snippet_safe(&ip) || !super::hooksignal::snippet_safe(&fp) { return Err("The data folder path has characters the snippet cannot carry safely".into()); }
    let cmd = format!("[ -f \"{fp}\" ] && cat \"{ip}\" || true");
    let hook = serde_json::to_string_pretty(&serde_json::json!({"hooks": {"UserPromptSubmit": [{"hooks": [{"type": "command", "command": cmd}]}]}})).map_err(|_| "snippet failed")?;
    Ok(AwayStatus { away: since.is_some(), since, pending, instruction: instruction_text(&fs), hook_snippet: hook, file: fs })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn parked_questions_fold_and_hide() {
        let t = "{\"t\":10,\"id\":\"Q-1\",\"ev\":\"ask\",\"f\":\"worker\",\"p\":\"app\",\"q\":\"Use A or B?\",\"o\":[\"A\",\"B\"]}\n{\"t\":20,\"id\":\"Q-2\",\"ev\":\"ask\",\"f\":\"w2\",\"q\":\"Push now?\"}\n{\"t\":30,\"id\":\"Q-2\",\"ev\":\"answered\"}\n{\"t\":40,\"id\":\"Q-3\",\"ev\":\"ask\",\"q\":\"x\\u0007y\"}\nnot json\n{\"t\":1,\"id\":\"bad id\",\"ev\":\"ask\",\"q\":\"x\"}\n{\"t\":1,\"id\":\"Q-9\",\"ev\":\"ask\"}\n";
        let v = fold(t, 0);
        assert_eq!(v.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["Q-1", "Q-3"], "answered, nameless-question and bad lines are gone");
        assert_eq!(v[0].options, ["A", "B"]); assert_eq!(v[1].question, "xy");
        assert_eq!(fold(t, 10).iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["Q-3"], "seen hides older");
    }
    #[test] fn instruction_names_the_file_and_forbids_approvals() {
        let s = instruction_text("/data/decisions/pending.jsonl");
        assert!(s.contains(">> \"/data/decisions/pending.jsonl\"") && s.contains("not an approval") && s.contains("needs the person's approval"));
        assert!(!s.contains("\\n"), "real newlines");
    }
}
