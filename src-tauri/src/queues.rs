//! Resource queues (a GPU queue, build slots, anything a scheduler keeps) shown read-only. A scheduler or an adapter script writes one JSON line per event to
//! `queues/<name>.jsonl` in this app's private folder; the newest line per id wins. The app only reads, never schedules, starts, stops or reorders anything.
//! Line: `{t, id, ev, who, est_min?, slot?, pos?, since?, until?, note?}`; ev is queued, running, reserved, done or released.
use serde::Serialize;
use std::collections::HashMap;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::PathBuf;

const MAX_QUEUES: usize = 10;
const MAX_ITEMS: usize = 100;
const AGE_S: i64 = 24 * 3600;

pub fn dir() -> Result<PathBuf, String> {
    let d = super::app_data_dir()?.join("queues");
    match std::fs::DirBuilder::new().mode(0o700).create(&d) { Ok(()) => (), Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (), Err(_) => return Err("Cannot create the queues folder".into()) }
    let m = std::fs::symlink_metadata(&d).map_err(|_| "Cannot inspect the queues folder")?;
    if !m.is_dir() || m.uid() != agent_app_core::sys::euid() || m.mode() & 0o077 != 0 { return Err("The queues folder must be private and owned by you".into()); }
    Ok(d)
}
fn clean(s: &str, max: usize) -> String { s.chars().filter(|c| !c.is_control()).take(max).collect() }

#[derive(Debug, Clone, Serialize)]
pub struct Item { pub id: String, pub who: String, pub est_min: Option<i64>, pub slot: String, pub pos: Option<i64>, pub t: i64, pub until: Option<i64>, pub note: String, pub over: bool, pub waited_s: Option<i64> }
#[derive(Debug, Serialize, Default)]
pub struct QueueView { pub name: String, pub running: Vec<Item>, pub waiting: Vec<Item>, pub reserved: Vec<Item>, pub last_t: i64 }

struct Ev { t: i64, ev: String, item: Item }
fn parse(line: &str) -> Option<Ev> {
    if line.len() > 4096 { return None; }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let t = v.get("t")?.as_i64().filter(|t| *t > 0)?;
    let id = v.get("id")?.as_str().filter(|s| super::mailbox::safe_name(s))?.to_string();
    let ev = v.get("ev")?.as_str().filter(|e| ["queued", "running", "reserved", "done", "released"].contains(e))?.to_string();
    let txt = |k: &str, n: usize| v.get(k).and_then(|x| x.as_str()).map(|s| agent_app_core::policy::redact(&clean(s, n))).unwrap_or_default();
    let num = |k: &str| v.get(k).and_then(|x| x.as_i64()).filter(|n| (0..=100_000_000_000).contains(n));
    let since = num("since");
    Some(Ev { t, ev, item: Item { id, who: txt("who", 40), est_min: num("est_min").filter(|n| *n <= 100_000), slot: txt("slot", 12), pos: num("pos"), t: since.unwrap_or(t), until: num("until"), note: txt("note", 160), over: false, waited_s: since.map(|s| (t - s).max(0)) } })
}
/// Folds events into the current state at time `now`.
pub fn fold(text: &str, name: &str, now: i64) -> QueueView {
    let mut cur: HashMap<String, Ev> = HashMap::new();
    for l in text.lines() { if let Some(e) = parse(l) { cur.insert(e.item.id.clone(), e); } }
    let mut q = QueueView { name: name.into(), ..Default::default() };
    let mut run: Vec<Ev> = cur.into_values().filter(|e| e.ev != "done" && e.ev != "released" && now - e.t < AGE_S).collect();
    q.last_t = run.iter().map(|e| e.t).max().unwrap_or(0);
    run.sort_by(|a, b| a.item.pos.unwrap_or(i64::MAX).cmp(&b.item.pos.unwrap_or(i64::MAX)).then(a.item.t.cmp(&b.item.t)).then(a.item.id.cmp(&b.item.id)));
    // a newer running line in the same slot ends the older one (the end line is sometimes lost)
    let mut newest: HashMap<String, i64> = HashMap::new();
    for e in run.iter().filter(|e| e.ev == "running" && !e.item.slot.is_empty()) { let n = newest.entry(e.item.slot.clone()).or_insert(0); *n = (*n).max(e.item.t); }
    for e in run {
        let mut it = e.item;
        match e.ev.as_str() {
            "running" => { if !it.slot.is_empty() && newest.get(&it.slot).is_some_and(|n| *n > it.t) { continue; } it.over = it.est_min.is_some_and(|m| m > 0 && now - it.t > m * 60); q.running.push(it); }
            "queued" => q.waiting.push(it),
            "reserved" => { if it.until.is_some_and(|u| u <= now) { continue; } q.reserved.push(it); }
            _ => {}
        }
    }
    for it in &mut q.waiting { if it.waited_s.is_some() { it.waited_s = Some(now - it.t); } }
    q.running.truncate(MAX_ITEMS); q.waiting.truncate(MAX_ITEMS); q.reserved.truncate(MAX_ITEMS);
    q
}
fn now_s() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) }

/// Every queue file with something in it (none: an empty list, so the page keeps the card hidden).
#[tauri::command]
pub fn queues_view(window: tauri::WebviewWindow) -> Result<Vec<QueueView>, String> {
    super::authorized(&window)?;
    let d = dir()?; let now = now_s(); let mut out = Vec::new();
    for e in std::fs::read_dir(&d).map_err(|_| "Cannot read the queues folder")?.flatten().take(MAX_QUEUES * 3) {
        let n = e.file_name().to_string_lossy().into_owned();
        let Some(name) = n.strip_suffix(".jsonl") else { continue };
        if !super::mailbox::safe_name(name) { continue; }
        let q = fold(&super::kanban::read_tail(&e.path()), name, now);
        if !(q.running.is_empty() && q.waiting.is_empty() && q.reserved.is_empty()) { out.push(q); }
        if out.len() >= MAX_QUEUES { break; }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn folds_running_waiting_reserved_and_marks_over_estimate() {
        let now = 100_000;
        let t = format!("{}\n{}\n{}\n{}\n{}\n{}\n",
            r#"{"t":99000,"id":"r1","ev":"running","who":"a","est_min":10,"slot":"0"}"#,
            r#"{"t":99900,"id":"r2","ev":"running","who":"b","est_min":30,"slot":"1"}"#,
            r#"{"t":99500,"id":"w2","ev":"queued","who":"c","est_min":5,"pos":2,"since":99400}"#,
            r#"{"t":99500,"id":"w1","ev":"queued","who":"d","est_min":50,"pos":1,"since":99000}"#,
            r#"{"t":99950,"id":"res","ev":"reserved","who":"owner","note":"quiet run","until":100600}"#,
            r#"{"t":99960,"id":"gone","ev":"queued","who":"x"}"#);
        let q = fold(&(t + "{\"t\":99970,\"id\":\"gone\",\"ev\":\"done\"}\n"), "gpu", now);
        assert_eq!(q.running.len(), 2); assert!(q.running.iter().find(|i| i.id == "r1").unwrap().over, "1000 s > 10 min");
        assert!(!q.running.iter().find(|i| i.id == "r2").unwrap().over);
        assert_eq!(q.waiting.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["w1", "w2"], "ordered by position");
        assert_eq!(q.waiting[0].waited_s, Some(1000)); assert_eq!(q.reserved.len(), 1);
        assert!(fold(r#"{"t":99950,"id":"res","ev":"reserved","until":99999}"#, "g", now).reserved.is_empty(), "expired reservation hidden");
    }
    #[test] fn newer_run_in_a_slot_ends_the_older_and_bad_lines_are_ignored() {
        let t = "{\"t\":10,\"id\":\"a\",\"ev\":\"running\",\"slot\":\"0\"}\n{\"t\":20,\"id\":\"b\",\"ev\":\"running\",\"slot\":\"0\"}\nnot json\n{\"t\":1,\"id\":\"bad id\",\"ev\":\"running\"}\n{\"t\":1,\"id\":\"z\",\"ev\":\"nope\"}\n";
        let q = fold(t, "q", 30); assert_eq!(q.running.len(), 1); assert_eq!(q.running[0].id, "b");
        assert!(fold("{\"t\":1,\"id\":\"old\",\"ev\":\"running\"}", "q", 1 + 25 * 3600).running.is_empty(), "older than a day dropped");
    }
}
