//! A timeline of tasks (one task = one prompt you sent until Claude stopped) and a list of Claude Code workflow runs.
//! Tasks: the person pastes two small Claude Code hooks (UserPromptSubmit = start, Stop = end); each appends one JSON line to a
//! file in this app's private signals folder. The app only READS that file. The label is the first line of the prompt, redacted and
//! cut to 80 characters. Tokens per finished task: both hook lines carry `tp` (Claude's `transcript_path`). The app opens that file only
//! if it is an absolute `.jsonl` path inside `<Claude root>/projects` with no symlink on the way, reads at most 32 MB of it, and sums
//! the four numeric `message.usage` fields of the lines whose timestamp lies in the task window. Nothing else in a line is deserialised
//! (no content), and neither the path nor any transcript text goes to the page or to a log.
//! Workflow runs: Claude Code writes one record per run (`projects/<slug>/<session>/workflows/wf_*.json`). This on-disk format is
//! observed, not specified, and may change. Only run id, state, agent count, start, duration and token total are read from it;
//! prompts, scripts, logs and agent outputs are never kept or shown.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const TAIL_BYTES: u64 = 256 * 1024;
const MAX_LINE: usize = 4096;
const MAX_ROWS: usize = 300;
const LABEL_CHARS: usize = 80;
/// A start with no end after this long is shown as "no end logged" rather than "running".
const RUNNING_SECS: i64 = 12 * 3600;
const RUN_FILE_CAP: u64 = 8 * 1024 * 1024;
pub const TRANSCRIPT_CAP: u64 = 32 * 1024 * 1024;
const MAX_TRANSCRIPTS: usize = 25;
const MAX_USES: usize = 400_000;
const MAX_RUNS: usize = 40;
const MAX_SCAN: usize = 6000;
const MAX_READS: usize = 200;

pub const TOKENS_NOTE: &str = "Tokens come from the numeric usage fields of the assistant lines in Claude's transcript (path logged by the hook as tp) whose time falls between the task's start and end. Cached input is counted separately. n/a means the number could not be read; nothing is estimated.";

/// Token counts of one task. Cache reads are kept apart from fresh input and output, never added silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct Tokens { pub input: u64, pub output: u64, pub cache_creation: u64, pub cache_read: u64 }

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Task { pub id: String, pub session: String, pub project: String, pub start: i64, pub end: Option<i64>, pub duration: Option<i64>, pub state: String, pub label: String, pub tokens: Option<Tokens>, pub tokens_why: String, #[serde(skip)] pub tp: Option<String> }
#[derive(Serialize)]
pub struct TasksReport { pub rows: Vec<Task>, pub tokens_note: String }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event { t: i64, session: String, project: String, start: bool, label: String, tp: Option<String> }

pub fn file() -> Result<PathBuf, String> { Ok(super::hooksignal::dir()?.join("claude-tasks.jsonl")) }

/// True for a word that looks like a path and names a denied folder or file anywhere in it. A word looks like a path when, after
/// quotes, brackets and trailing punctuation around it are dropped, it contains `/` or `\` or starts with `$`, `.` or `~`
/// (so `(/x/.ssh/id_rsa)`, `foo/.ssh/key`, `./.env`, `$HOME/.aws/credentials` and Windows paths with backslashes all count). Backslashes
/// are read as folder separators.
pub(crate) fn path_word_denied(w: &str, deny: &[String]) -> bool {
    let t = w.trim_matches(|c: char| matches!(c, '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '"' | '\'' | '`' | ',' | ';' | ':' | '!' | '?'));
    if !(t.contains('/') || t.contains('\\') || t.starts_with('$') || t.starts_with('.') || t.starts_with('~')) { return false; }
    let n = t.replace('\\', "/");
    let trimmed = n.trim_end_matches(['.', ',', ';', ':']);
    agent_app_core::policy::denied_name(&n, deny) || agent_app_core::policy::denied_name(trimmed, deny)
}

pub(crate) fn label_of(m: &str, deny: &[String]) -> String {
    let first = m.lines().next().unwrap_or("");
    let clean: String = first.chars().filter(|c| !c.is_control()).collect();
    // A path-like word that names a denied folder or file is replaced as a whole.
    let masked: Vec<&str> = clean.split_whitespace().map(|w| if path_word_denied(w, deny) { "[hidden]" } else { w }).collect();
    // Redact the whole line first, then cut: cutting first could split a key so the pattern no longer matches.
    let l: String = agent_app_core::policy::redact(&masked.join(" ")).chars().take(LABEL_CHARS).collect();
    if l.is_empty() { "(no text)".into() } else { l }
}

pub fn parse_line(line: &str, deny: &[String]) -> Option<Event> {
    if line.len() > MAX_LINE { return None; }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let t = v.get("t")?.as_i64().filter(|t| *t > 0)?;
    let session = v.get("s")?.as_str().filter(|s| super::hooksignal::safe_id(s))?.to_string();
    let start = match v.get("k")?.as_str()? { "start" => true, "end" => false, _ => return None };
    let cwd = super::hooksignal::clean(v.get("c").and_then(|c| c.as_str()).unwrap_or(""), 300);
    let hidden = agent_app_core::policy::denied_name(&cwd, deny);
    let project = if hidden { String::new() } else { Path::new(&cwd).file_name().map(|n| agent_app_core::policy::redact(&n.to_string_lossy())).unwrap_or_default() };
    let label = if start { label_of(v.get("m").and_then(|m| m.as_str()).unwrap_or(""), deny) } else { String::new() };
    let tp = v.get("tp").and_then(|p| p.as_str()).filter(|p| p.len() <= 600 && !p.contains(|c: char| c.is_control())).map(str::to_string);
    Some(Event { t, session, project, start, label, tp })
}

/// Pairs start and end events per session, in the order given (oldest first). A second start with no end in between leaves the first
/// task as "no end logged" (an interrupted prompt fires no Stop hook). The last open task is "running" while it is recent.
pub fn pair(events: &[Event], now: i64) -> Vec<Task> {
    let mut open: HashMap<&str, usize> = HashMap::new();
    let mut tasks: Vec<Task> = Vec::new();
    for e in events {
        if e.start {
            open.insert(&e.session, tasks.len());
            tasks.push(Task { id: format!("{}:{}", e.session, e.t), session: e.session.clone(), project: e.project.clone(), start: e.t, end: None, duration: None, state: "running".into(), label: e.label.clone(), tokens: None, tokens_why: String::new(), tp: e.tp.clone() });
        } else if let Some(i) = open.remove(e.session.as_str()) {
            let t = &mut tasks[i];
            if e.t >= t.start { if e.tp.is_some() { t.tp = e.tp.clone(); } t.end = Some(e.t); t.duration = Some(e.t - t.start); t.state = "done".into(); }
        }
    }
    let current: Vec<usize> = open.values().copied().collect();
    for (i, t) in tasks.iter_mut().enumerate().filter(|(_, t)| t.end.is_none()) {
        t.state = if current.contains(&i) && now - t.start <= RUNNING_SECS { "running" } else { "no end logged" }.into();
    }
    tasks
}

/// The newest tasks from the end of the file (at most `limit`, newest first), optionally for one session. Refuses symlinks.
pub fn read_recent(path: &Path, deny: &[String], session: Option<&str>, limit: usize, now: i64) -> Vec<Task> {
    let Ok(mut f) = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path) else { return vec![] };
    let Ok(meta) = f.metadata() else { return vec![] };
    if !meta.is_file() { return vec![]; }
    let start = meta.len().saturating_sub(TAIL_BYTES);
    // From the second tail byte on, one byte earlier is read: if it is a newline the first line is whole, otherwise it is cut.
    let from = start.saturating_sub(1);
    if f.seek(SeekFrom::Start(from)).is_err() { return vec![]; }
    let mut buf = Vec::new();
    if (&mut f).take(TAIL_BYTES + (start - from)).read_to_end(&mut buf).is_err() { return vec![]; }
    let body: &[u8] = if start == 0 { &buf } else if buf.first() == Some(&b'\n') { &buf[1..] } else { match buf.iter().position(|b| *b == b'\n') { Some(i) => &buf[i + 1..], None => &[] } };
    let text = String::from_utf8_lossy(body);
    // A last line without a newline is kept when it is whole JSON; an unfinished one fails to parse and is dropped there.
    let lines: Vec<&str> = text.lines().collect();
    let events: Vec<Event> = lines.into_iter().filter_map(|l| parse_line(l, deny)).filter(|e| session.is_none_or(|s| e.session == s)).collect();
    let mut rows = pair(&events, now);
    rows.sort_by(|a, b| b.start.cmp(&a.start));
    rows.truncate(limit.clamp(1, MAX_ROWS));
    rows
}

fn now_s() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) }


// ---- Tokens per finished task, from the transcript the hook named ----

/// Only these fields are deserialised from a transcript line: the timestamp, the ids and the four usage numbers. No content field exists here.
#[derive(Deserialize)]
struct RawLine { timestamp: Option<String>, #[serde(rename = "sessionId")] session_id: Option<String>, message: Option<RawMsg> }
#[derive(Deserialize)]
struct RawMsg { id: Option<String>, usage: Option<RawUsage> }
#[derive(Deserialize)]
struct RawUsage { input_tokens: Option<u64>, output_tokens: Option<u64>, cache_creation_input_tokens: Option<u64>, cache_read_input_tokens: Option<u64> }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Use { t: i64, session: Option<String>, tok: Tokens }
#[derive(Debug, Default)]
pub struct Loaded { uses: Vec<Use>, first_t: Option<i64>, pub(crate) tail_only: bool }

/// "2026-11-11T07:50:39.626Z" to whole seconds since the epoch (UTC; any other offset is refused).
pub fn parse_ts(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' || !s.ends_with('Z') { return None; }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 || y < 1970 { return None; }
    let (y2, m2) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = y2 / 400; let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + d - 1; let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146097 + doe - 719468) * 86400 + h * 3600 + mi * 60 + se)
}

/// Parses transcript bytes line by line. Malformed lines and lines without a timestamp or usage are skipped; returns how many were skipped.
/// A message id seen twice (one line per content block) counts once, with the later numbers.
pub fn load_usage(bytes: &[u8], tail_only: bool) -> (Loaded, usize) {
    let mut out = Loaded { tail_only, ..Default::default() };
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut skipped = 0usize;
    for line in bytes.split(|b| *b == b'\n') {
        if line.iter().all(|b| b.is_ascii_whitespace()) { continue; }
        let Ok(l) = serde_json::from_slice::<RawLine>(line) else { skipped += 1; continue };
        let Some(t) = l.timestamp.as_deref().and_then(parse_ts) else { continue };
        out.first_t = Some(out.first_t.map_or(t, |f| f.min(t)));
        let Some(m) = l.message else { continue };
        let Some(u) = m.usage else { continue };
        let tok = Tokens { input: u.input_tokens.unwrap_or(0), output: u.output_tokens.unwrap_or(0), cache_creation: u.cache_creation_input_tokens.unwrap_or(0), cache_read: u.cache_read_input_tokens.unwrap_or(0) };
        let id = m.id.filter(|i| i.len() <= 100);
        if let Some(&i) = id.as_ref().and_then(|i| seen.get(i)) { out.uses[i].tok = tok; continue; }
        if out.uses.len() >= MAX_USES { continue; }
        if let Some(i) = id { seen.insert(i, out.uses.len()); }
        out.uses.push(Use { t, session: l.session_id.filter(|s| s.len() <= 100), tok });
    }
    (out, skipped)
}

/// Sum over lines with start <= time <= end that belong to `session` (lines naming another session are left out).
pub fn sum_window(l: &Loaded, session: &str, start: i64, end: i64) -> Tokens {
    let mut a = Tokens::default();
    for u in l.uses.iter().filter(|u| u.t >= start && u.t <= end && u.session.as_deref().is_none_or(|s| s == session)) {
        a.input = a.input.saturating_add(u.tok.input); a.output = a.output.saturating_add(u.tok.output); a.cache_creation = a.cache_creation.saturating_add(u.tok.cache_creation); a.cache_read = a.cache_read.saturating_add(u.tok.cache_read);
    }
    a
}

/// Sum of every usage line (a subagent's own transcript belongs to that subagent as a whole).
pub(crate) fn sum_all(l: &Loaded) -> Tokens {
    let mut a = Tokens::default();
    for u in &l.uses { a.input = a.input.saturating_add(u.tok.input); a.output = a.output.saturating_add(u.tok.output); a.cache_creation = a.cache_creation.saturating_add(u.tok.cache_creation); a.cache_read = a.cache_read.saturating_add(u.tok.cache_read); }
    a
}

/// Opens a transcript only if `tp` is an absolute .jsonl path inside `<root>/projects`, at least `<slug>/<file>` deep, with no symlink on
/// the way and a regular file at the end. The Err text is shown on the page instead of the number; it never contains the path.
pub fn open_transcript(root: &Path, tp: &str) -> Result<std::fs::File, &'static str> {
    use std::path::Component;
    let p = Path::new(tp);
    if !p.is_absolute() || !tp.ends_with(".jsonl") || tp.contains('\0') { return Err("the hook logged a transcript path that is not an absolute .jsonl path"); }
    let lexical = root.join("projects");
    let proot = std::fs::canonicalize(&lexical).map_err(|_| "Claude's projects folder was not found")?;
    let rel = p.strip_prefix(&proot).or_else(|_| p.strip_prefix(&lexical)).map_err(|_| "the transcript path is outside Claude's projects folder")?;
    let comps: Vec<Component> = rel.components().collect();
    if comps.len() < 2 || !comps.iter().all(|c| matches!(c, Component::Normal(_))) { return Err("the transcript path is not <project>/<file>.jsonl inside Claude's projects folder"); }
    let mut cur = proot;
    for c in comps {
        cur.push(c);
        let m = std::fs::symlink_metadata(&cur).map_err(|_| "the transcript file is missing")?;
        if m.file_type().is_symlink() { return Err("the transcript path goes through a symlink, which is refused"); }
    }
    let f = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(&cur).map_err(|_| "the transcript file could not be opened")?;
    if !f.metadata().map(|m| m.is_file()).unwrap_or(false) { return Err("the transcript is not a regular file"); }
    Ok(f)
}

/// Reads at most TRANSCRIPT_CAP bytes (the end of a bigger file, from a line boundary) and returns its usage lines.
fn read_transcript(root: &Path, tp: &str) -> Result<Loaded, &'static str> {
    let mut f = open_transcript(root, tp)?;
    let len = f.metadata().map_err(|_| "the transcript file could not be read")?.len();
    let tail_only = len > TRANSCRIPT_CAP;
    let from = len.saturating_sub(TRANSCRIPT_CAP).saturating_sub(if tail_only { 1 } else { 0 });
    f.seek(SeekFrom::Start(from)).map_err(|_| "the transcript file could not be read")?;
    let mut buf = Vec::new();
    (&mut f).take(TRANSCRIPT_CAP + 1).read_to_end(&mut buf).map_err(|_| "the transcript file could not be read")?;
    let body: &[u8] = if !tail_only { &buf } else if buf.first() == Some(&b'\n') { &buf[1..] } else { buf.iter().position(|b| *b == b'\n').map_or(&[][..], |i| &buf[i + 1..]) };
    Ok(load_usage(body, tail_only).0)
}

type CacheKey = (PathBuf, u64, Option<std::time::SystemTime>);
/// At least as many as the tasks report (25 transcripts) and the subagents report (their newest transcripts) read together, so a
/// 5 s refresh of the Flow page does not re-read them all.
const CACHE_ENTRIES: usize = 48;
const _: () = assert!(CACHE_ENTRIES >= MAX_TRANSCRIPTS + super::subagents::MAX_TOKEN_READS);
static CACHE: std::sync::Mutex<Vec<(CacheKey, std::sync::Arc<Loaded>)>> = std::sync::Mutex::new(Vec::new());

pub(crate) fn load_cached(root: &Path, tp: &str) -> Result<std::sync::Arc<Loaded>, &'static str> {
    let f = open_transcript(root, tp)?;
    let m = f.metadata().map_err(|_| "the transcript file could not be read")?;
    drop(f);
    let key: CacheKey = (PathBuf::from(tp), m.len(), m.modified().ok());
    if let Ok(c) = CACHE.lock() { if let Some((_, l)) = c.iter().find(|(k, _)| *k == key) { return Ok(l.clone()); } }
    let l = std::sync::Arc::new(read_transcript(root, tp)?);
    if let Ok(mut c) = CACHE.lock() { if c.len() >= CACHE_ENTRIES { c.remove(0); } c.push((key, l.clone())); }
    Ok(l)
}

/// Fills `tokens` (or `tokens_why`) of every finished task. Running and unfinished tasks get no number.
pub fn fill_tokens(rows: &mut [Task], root: Option<&Path>) {
    let mut seen: Vec<String> = Vec::new();
    for t in rows.iter_mut() {
        if t.end.is_none() { continue; }
        let Some(tp) = t.tp.clone() else { t.tokens_why = "the Stop hook did not log a transcript path (tp); update the hook snippet in Settings".into(); continue };
        let Some(root) = root else { t.tokens_why = "Claude's folder is unknown".into(); continue };
        if !seen.contains(&tp) { if seen.len() >= MAX_TRANSCRIPTS { t.tokens_why = "too many different transcripts to read at once".into(); continue; } seen.push(tp.clone()); }
        match load_cached(root, &tp) {
            Err(why) => t.tokens_why = why.into(),
            Ok(l) => {
                if l.tail_only && l.first_t.is_none_or(|f| f > t.start) { t.tokens_why = "the transcript is over 32 MB and this task starts before the part that was read".into(); continue; }
                t.tokens = Some(sum_window(&l, &t.session, t.start, t.end.unwrap_or(t.start)));
            }
        }
    }
}

#[derive(Serialize)]
pub struct TasksStatus { file: String, snippet: String, exists: bool, size: u64, last_at: Option<i64>, tokens_note: String }

#[tauri::command]
pub fn tasks_status(window: tauri::WebviewWindow) -> Result<TasksStatus, String> {
    super::authorized(&window)?;
    let path = file()?;
    let p = path.to_string_lossy().into_owned();
    if !super::hooksignal::snippet_safe(&p) { return Err("The signals folder path has characters the snippet cannot carry safely".into()); }
    // The hook keeps only the first line of the prompt, at most 200 characters; the app cuts it to 80 and redacts it again.
    let start = format!("jq -c '{{t:(now|floor),s:.session_id,c:.cwd,tp:.transcript_path,k:\"start\",m:(((.prompt // \"\")|split(\"\\n\")[0] // \"\")|.[0:200])}}' >> \"{p}\"");
    let end = format!("jq -c '{{t:(now|floor),s:.session_id,c:.cwd,tp:.transcript_path,k:\"end\"}}' >> \"{p}\"");
    let snippet = serde_json::to_string_pretty(&serde_json::json!({"hooks": {
        "UserPromptSubmit": [{"hooks": [{"type": "command", "command": start}]}],
        "Stop": [{"hooks": [{"type": "command", "command": end}]}]}})).map_err(|_| "snippet failed")?;
    let last = read_recent(&path, &agent_app_core::policy::builtin().deny, None, 1, now_s()).first().map(|t| t.start);
    Ok(TasksStatus { file: p, snippet, exists: path.is_file(), size: super::hooksignal::file_len(&path), last_at: last, tokens_note: TOKENS_NOTE.into() })
}

/// Newest tasks first. `session` narrows to one Claude session id; `limit` is capped. Tokens: see `TOKENS_NOTE`.
#[tauri::command(async)]
pub fn tasks_report(window: tauri::WebviewWindow, session: Option<String>, limit: Option<usize>) -> Result<TasksReport, String> {
    super::authorized(&window)?;
    if let Some(s) = &session { if !super::hooksignal::safe_id(s) { return Err("Bad session id".into()); } }
    let mut rows = read_recent(&file()?, &agent_app_core::policy::builtin().deny, session.as_deref(), limit.unwrap_or(150), now_s());
    fill_tokens(&mut rows, super::claude_settings::root().as_deref());
    Ok(TasksReport { rows, tokens_note: TOKENS_NOTE.into() })
}

// ---- Claude Code workflow runs (read-only, content-free) ----

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Run { pub id: String, pub project: String, pub name: String, pub state: String, pub agents: Option<u64>, pub start: Option<i64>, pub duration_ms: Option<u64>, pub tokens: Option<u64> }
/// `skipped`: record files that were found but could not be used (unreadable, too big, not the expected shape). `truncated`: the folder
/// limits were hit, so older records were not looked at.
#[derive(Debug, Default, Serialize)]
pub struct RunsReport { pub runs: Vec<Run>, pub skipped: usize, pub truncated: bool, pub note: &'static str }
pub const RUNS_NOTE: &str = "Claude Code's workflow run records are an observed on-disk format, not a documented one; it may change, and records that no longer match are counted as skipped.";

/// Only these fields are read; serde skips the rest (script, result, logs, summary, progress) without keeping it.
#[derive(Deserialize)]
struct RawRun {
    #[serde(rename = "runId")] run_id: Option<String>,
    #[serde(rename = "workflowName")] name: Option<String>,
    status: Option<String>,
    #[serde(rename = "agentCount")] agents: Option<u64>,
    #[serde(rename = "startTime")] start_ms: Option<i64>,
    #[serde(rename = "durationMs")] duration_ms: Option<u64>,
    #[serde(rename = "totalTokens")] tokens: Option<u64>,
}

fn run_id_ok(s: &str) -> bool { s.starts_with("wf_") && s.len() <= 40 && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')) }
pub(crate) fn dir_name_ok(s: &str) -> bool { !s.is_empty() && s.len() <= 200 && !s.starts_with('.') && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')) }

pub fn parse_run(bytes: &[u8], file_stem: &str, project: &str) -> Option<Run> {
    let r: RawRun = serde_json::from_slice(bytes).ok()?;
    let id = r.run_id.filter(|i| run_id_ok(i))?;
    if id != file_stem { return None; }
    let state = match r.status.as_deref() { Some(s @ ("completed" | "failed" | "killed" | "running")) => s.to_string(), _ => "unknown".into() };
    let name: String = agent_app_core::policy::redact(&r.name.unwrap_or_default().chars().filter(|c| !c.is_control()).collect::<String>()).chars().take(60).collect();
    let start = r.start_ms.filter(|ms| *ms > 0).map(|ms| ms / 1000);
    Some(Run { id, project: project.into(), name, state, agents: r.agents, start, duration_ms: r.duration_ms, tokens: r.tokens })
}

fn read_run_file(path: &Path) -> Option<Vec<u8>> {
    let f = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path).ok()?;
    let m = f.metadata().ok()?;
    if !m.is_file() || m.len() > RUN_FILE_CAP { return None; }
    let mut buf = Vec::new();
    f.take(RUN_FILE_CAP).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// The project name for a Claude folder slug ("-srv-a-proj" stands for the folder path with `-` in place of each `/`): the last segment only,
/// blank when any segment names a denied folder (also in its dotted form). The slug itself, the user name and the session folder never leave.
pub fn project_of(slug: &str, deny: &[String]) -> String {
    let segs: Vec<&str> = slug.split('-').filter(|s| !s.is_empty()).collect();
    let denied = segs.iter().any(|s| agent_app_core::policy::denied_name(s, deny) || agent_app_core::policy::denied_name(&format!(".{s}"), deny));
    if denied { return String::new(); }
    segs.last().map(|l| agent_app_core::policy::redact(l).chars().take(40).collect()).unwrap_or_default()
}

/// Directory entries that are real folders (no symlinks) with an acceptable name, newest modification first, at most `n`; the bool says more existed.
fn newest_dirs(p: &Path, n: usize) -> (Vec<(String, PathBuf)>, bool) {
    let Ok(rd) = std::fs::read_dir(p) else { return (vec![], false) };
    let mut v: Vec<(std::time::SystemTime, String, PathBuf)> = rd.flatten().take(20_000).filter_map(|e| {
        let name = e.file_name().into_string().ok().filter(|n| dir_name_ok(n))?;
        let m = std::fs::symlink_metadata(e.path()).ok().filter(|m| m.is_dir())?;
        Some((m.modified().unwrap_or(std::time::UNIX_EPOCH), name, e.path())) }).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    let more = v.len() > n; v.truncate(n);
    (v.into_iter().map(|(_, n, p)| (n, p)).collect(), more)
}

/// Newest runs (by file time, not directory order) under `<root>/projects/<slug>/<session>/workflows/wf_*.json`. Symlinked folders and
/// files are skipped. Files that were looked at but could not be used are counted in `skipped`.
pub fn scan_runs(root: &Path, limit: usize, deny: &[String]) -> RunsReport {
    let mut rep = RunsReport { note: RUNS_NOTE, ..Default::default() };
    let mut found: Vec<(std::time::SystemTime, PathBuf, String, String)> = Vec::new();
    let mut scanned = 0usize;
    let (slugs, more) = newest_dirs(&root.join("projects"), 300);
    rep.truncated |= more;
    'outer: for (slug, sp) in slugs {
        let (sessions, more) = newest_dirs(&sp, 2000); rep.truncated |= more;
        for (_, se) in sessions {
            scanned += 1; if scanned > MAX_SCAN { rep.truncated = true; break 'outer; }
            let wd = se.join("workflows");
            if !std::fs::symlink_metadata(&wd).map(|m| m.is_dir()).unwrap_or(false) { continue; }
            let Ok(files) = std::fs::read_dir(&wd) else { continue };
            let mut here: Vec<(std::time::SystemTime, PathBuf, String)> = Vec::new();
            for f in files.flatten().take(5000) {
                let Ok(name) = f.file_name().into_string() else { continue };
                let Some(stem) = name.strip_suffix(".json").filter(|s| run_id_ok(s)) else { continue };
                let Ok(m) = std::fs::symlink_metadata(f.path()) else { continue };
                if !m.is_file() { continue; }
                here.push((m.modified().unwrap_or(std::time::UNIX_EPOCH), f.path(), stem.to_string()));
            }
            here.sort_by(|a, b| b.0.cmp(&a.0));
            if here.len() > 500 { rep.truncated = true; here.truncate(500); }
            found.extend(here.into_iter().map(|(t, p, stem)| (t, p, stem, slug.clone())));
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    let want = limit.clamp(1, MAX_RUNS);
    for (i, (_, path, stem, slug)) in found.into_iter().enumerate() {
        if rep.runs.len() >= want { break; }
        if i >= MAX_READS { rep.truncated = true; break; }
        match read_run_file(&path).and_then(|b| parse_run(&b, &stem, &project_of(&slug, deny))) { Some(r) => rep.runs.push(r), None => rep.skipped += 1 }
    }
    rep.runs.sort_by(|a, b| b.start.cmp(&a.start));
    rep
}

/// Claude Code workflow runs, newest first: id, state, agent count, start, duration and token total only. Never prompts or outputs.
#[tauri::command(async)]
pub fn workflows_report(window: tauri::WebviewWindow, limit: Option<usize>) -> Result<RunsReport, String> {
    super::authorized(&window)?;
    let root = super::claude_settings::root().ok_or("Home folder unknown")?;
    Ok(scan_runs(&root, limit.unwrap_or(30), &agent_app_core::policy::builtin().deny))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn deny() -> Vec<String> { agent_app_core::policy::builtin().deny }
    fn ev(s: &str) -> Event { parse_line(s, &deny()).unwrap() }
    fn start(t: i64, s: &str, m: &str) -> String { format!(r#"{{"t":{t},"s":"{s}","c":"/srv/example-project","k":"start","m":"{m}"}}"#) }
    fn end(t: i64, s: &str) -> String { format!(r#"{{"t":{t},"s":"{s}","c":"/srv/example-project","k":"end"}}"#) }

    #[test] fn pairs_start_and_end_per_session_with_interleaving() {
        let evs: Vec<Event> = [start(100, "a", "one"), start(110, "b", "two"), end(150, "a"), end(200, "b"), start(300, "a", "three")].iter().map(|l| ev(l)).collect();
        let t = pair(&evs, 320);
        assert_eq!(t.len(), 3);
        assert_eq!((t[0].label.as_str(), t[0].duration, t[0].state.as_str(), t[0].project.as_str()), ("one", Some(50), "done", "example-project"));
        assert_eq!((t[1].label.as_str(), t[1].duration), ("two", Some(90)));
        assert_eq!((t[2].end, t[2].state.as_str()), (None, "running"));
        assert!(t.iter().all(|x| x.tokens.is_none() && x.tp.is_none()));
    }
    #[test] fn a_start_without_an_end_and_a_stale_open_task() {
        let evs: Vec<Event> = [start(100, "a", "interrupted"), start(200, "a", "next"), end(210, "orphan"), end(250, "a")].iter().map(|l| ev(l)).collect();
        let t = pair(&evs, 260);
        assert_eq!((t[0].state.as_str(), t[0].end), ("no end logged", None));
        assert_eq!((t[1].state.as_str(), t[1].duration), ("done", Some(50)));
        let old = pair(&[ev(&start(1000, "a", "old"))], 1000 + RUNNING_SECS + 1);
        assert_eq!(old[0].state, "no end logged");
        let neg = pair(&[ev(&start(500, "a", "x")), ev(&end(400, "a"))], 600);
        assert_eq!(neg[0].end, None);
    }
    #[test] fn label_is_first_line_cut_to_80_redacted_and_clean() {
        let a = ev(&format!(r#"{{"t":5,"s":"a","c":"/p","k":"start","m":"{}"}}"#, "x".repeat(300)));
        assert_eq!(a.label.chars().count(), 80);
        let b = ev(r#"{"t":5,"s":"a","c":"/p","k":"start","m":"first\nsecond line"}"#); assert_eq!(b.label, "first");
        let c = ev("{\"t\":5,\"s\":\"a\",\"c\":\"/p\",\"k\":\"start\",\"m\":\"a\\u0007b\\u001bc\"}"); assert_eq!(c.label, "abc");
        let d = ev(r#"{"t":5,"s":"a","c":"/p","k":"start"}"#); assert_eq!(d.label, "(no text)");
    }
    #[test] fn key_shaped_text_in_a_prompt_is_redacted_even_at_the_cut() {
        let tok = format!("sk-ant-{}", "a1b2c3d4e5".repeat(4)); // built at run time so no key-shaped literal is in the source
        let a = ev(&format!(r#"{{"t":5,"s":"a","c":"/p","k":"start","m":"please use {tok} now"}}"#));
        assert!(!a.label.contains(&tok) && !a.label.contains("sk-ant-a1"), "{}", a.label);
        let long = format!("{}{tok}", "y".repeat(70)); // the key straddles character 80
        let b = ev(&format!(r#"{{"t":5,"s":"a","c":"/p","k":"start","m":"{long}"}}"#));
        assert!(!b.label.contains("sk-ant"), "{}", b.label);
    }
    #[test] fn rejects_bad_lines() {
        for bad in ["", "nope", r#"{"t":0,"s":"a","k":"start"}"#, r#"{"t":1,"s":"bad id","k":"start"}"#, r#"{"t":1,"s":"a","k":"pause"}"#, r#"{"t":1,"s":"a"}"#, r#"{"s":"a","k":"end"}"#] {
            assert!(parse_line(bad, &deny()).is_none(), "{bad}");
        }
        assert!(parse_line(&format!(r#"{{"t":1,"s":"a","k":"start","m":"{}"}}"#, "y".repeat(5000)), &deny()).is_none());
    }
    #[test] fn a_denied_project_folder_is_not_named() {
        let a = ev(r#"{"t":5,"s":"a","c":"/srv/u/.ssh","k":"start","m":"x"}"#); assert_eq!(a.project, "");
    }
    #[test] fn reads_newest_first_filters_by_session_caps_and_survives_a_cut_tail_and_unfinished_line() {
        let d = tempfile::tempdir().unwrap(); let p = d.path().join("t.jsonl");
        let mut s = String::new();
        for i in 0..5i64 { let sess = if i % 2 == 0 { "even" } else { "odd" }; s += &start(100 + i * 10, sess, &format!("task {i}")); s.push('\n'); s += &end(105 + i * 10, sess); s.push('\n'); }
        s += r#"{"t":999,"s":"odd","c":"/p","k":"start","m":"unfini"#; // unfinished last line
        std::fs::write(&p, &s).unwrap();
        let all = read_recent(&p, &deny(), None, 100, 1000);
        assert_eq!(all.iter().map(|t| t.start).collect::<Vec<_>>(), vec![140, 130, 120, 110, 100]);
        assert!(all.iter().all(|t| t.state == "done"));
        assert_eq!(read_recent(&p, &deny(), Some("even"), 100, 1000).len(), 3);
        assert_eq!(read_recent(&p, &deny(), None, 2, 1000).len(), 2);
        assert_eq!(read_recent(&p, &deny(), None, 100_000, 1000).len(), 5);
    }
    #[test] fn a_cut_first_line_of_a_big_file_is_dropped() {
        let d = tempfile::tempdir().unwrap(); let p = d.path().join("big.jsonl");
        let mut s = String::new();
        while (s.len() as u64) < TAIL_BYTES + 5000 { s += &start(10, "old", "filler"); s.push('\n'); }
        s += &start(500, "new", "latest"); s.push('\n'); s += &end(520, "new"); s.push('\n');
        std::fs::write(&p, &s).unwrap();
        let t = read_recent(&p, &deny(), Some("new"), 10, 600);
        assert_eq!((t.len(), t[0].duration), (1, Some(20)));
    }
    #[test] fn symlinks_are_refused() {
        let d = tempfile::tempdir().unwrap(); let real = d.path().join("r"); std::fs::write(&real, format!("{}\n", start(1, "a", "x"))).unwrap();
        let link = d.path().join("l"); std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(read_recent(&link, &deny(), None, 10, 5).is_empty());
        assert_eq!(read_recent(&real, &deny(), None, 10, 5).len(), 1);
    }

    fn run_json(id: &str, extra: &str) -> String {
        format!(r#"{{"runId":"{id}","timestamp":"2026-11-11T07:50:39.626Z","taskId":"t","script":"SECRET-SCRIPT-BODY","result":["SECRET-OUTPUT"],"agentCount":8,"logs":["SECRET-LOG"],"durationMs":421656,"summary":"SECRET-SUMMARY","workflowName":"triage","status":"completed","startTime":1794383017926,"phases":[],"workflowProgress":[{{"prompt":"SECRET-PROMPT"}}],"totalTokens":639122,"totalToolCalls":3{extra}}}"#)
    }
    #[test] fn a_run_record_yields_only_the_content_free_fields() {
        let r = parse_run(run_json("wf_abc-123", "").as_bytes(), "wf_abc-123", "proj").unwrap();
        assert_eq!((r.state.as_str(), r.agents, r.start, r.duration_ms, r.tokens, r.name.as_str(), r.project.as_str()), ("completed", Some(8), Some(1794383017), Some(421656), Some(639122), "triage", "proj"));
        let j = serde_json::to_string(&r).unwrap();
        assert!(!j.contains("SECRET"), "{j}");
        assert!(parse_run(run_json("wf_abc-123", "").as_bytes(), "wf_other", "p").is_none(), "id must match the file name");
        assert!(parse_run(run_json("../evil", "").as_bytes(), "../evil", "p").is_none());
        assert!(parse_run(b"not json", "wf_x", "p").is_none());
        let odd = run_json("wf_q", "").replace("completed", "weird");
        assert_eq!(parse_run(odd.as_bytes(), "wf_q", "p").unwrap().state, "unknown");
    }
    #[test] fn scan_finds_runs_skips_symlinks_counts_skipped_and_prefers_newest() {
        let d = tempfile::tempdir().unwrap(); let root = d.path();
        let wd = root.join("projects/-srv-proj/sess-1/workflows"); std::fs::create_dir_all(&wd).unwrap();
        std::fs::write(wd.join("wf_one-1.json"), run_json("wf_one-1", "")).unwrap();
        std::fs::write(wd.join("wf_two-2.json"), run_json("wf_two-2", "").replace("1794383017926", "1794383099999")).unwrap();
        std::fs::write(wd.join("wf_bad-4.json"), "not json").unwrap();
        std::fs::write(wd.join("notes.json"), "{}").unwrap();
        std::os::unix::fs::symlink(wd.join("wf_one-1.json"), wd.join("wf_link-3.json")).unwrap();
        let other = root.join("projects/-srv-other"); std::fs::create_dir_all(&other).unwrap();
        std::os::unix::fs::symlink(root.join("projects/-srv-proj/sess-1"), other.join("sess-link")).unwrap();
        let rep = scan_runs(root, 30, &deny());
        assert_eq!(rep.runs.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["wf_two-2", "wf_one-1"]);
        assert_eq!(rep.skipped, 1, "the unparsable record is counted");
        assert!(rep.note.contains("not a documented one") && !rep.truncated);
        assert_eq!(scan_runs(root, 1, &deny()).runs.len(), 1);
        assert!(!serde_json::to_string(&rep).unwrap().contains("SECRET"));
        assert!(scan_runs(&root.join("missing"), 30, &deny()).runs.is_empty());
    }
    #[test] fn only_the_newest_record_files_are_kept_when_there_are_more_than_the_limit() {
        let d = tempfile::tempdir().unwrap(); let wd = d.path().join("projects/-srv-proj/s/workflows"); std::fs::create_dir_all(&wd).unwrap();
        for i in 0..6u64 {
            let p = wd.join(format!("wf_r{i}.json")); std::fs::write(&p, run_json(&format!("wf_r{i}"), "")).unwrap();
            let f = std::fs::File::options().write(true).open(&p).unwrap(); f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000 + i * 100)).unwrap();
        }
        let ids: Vec<String> = scan_runs(d.path(), 2, &deny()).runs.into_iter().map(|r| r.id).collect();
        let mut ids = ids; ids.sort();
        assert_eq!(ids, vec!["wf_r4", "wf_r5"], "newest by modification time, not by directory order");
    }
    #[test] fn project_names_are_the_last_segment_and_blank_for_denied_folders() {
        assert_eq!(project_of("-home-alice-src-myproj", &deny()), "myproj");
        assert_eq!(project_of("-home-alice-work-myproj", &deny()), "", "a deny-listed folder in the slug blanks the name");
        assert_eq!(project_of("--config-thing", &deny()), "", "the dotted form of a denied name is caught");
        assert_eq!(project_of("-home-alice-ssh-keys", &deny()), "", "ssh may be .ssh");
        let j = serde_json::to_string(&project_of("-home-alice-src-myproj", &deny())).unwrap();
        assert!(!j.contains("alice") && !j.contains("home"));
        let d = tempfile::tempdir().unwrap(); let wd = d.path().join("projects/-home-alice-work-secret/sess-9/workflows"); std::fs::create_dir_all(&wd).unwrap();
        std::fs::write(wd.join("wf_a.json"), run_json("wf_a", "")).unwrap();
        let wd2 = d.path().join("projects/-home-alice-src-shown/sess-8/workflows"); std::fs::create_dir_all(&wd2).unwrap();
        std::fs::write(wd2.join("wf_b.json"), run_json("wf_b", "")).unwrap();
        let rep = scan_runs(d.path(), 10, &deny()); let j = serde_json::to_string(&rep).unwrap();
        assert!(!j.contains("alice") && !j.contains("-work-") && !j.contains("secret") && !j.contains("sess-9") && !j.contains("sess-8") && !j.contains("home"), "{j}");
        assert_eq!(rep.runs.iter().map(|r| r.project.as_str()).collect::<std::collections::BTreeSet<_>>(), ["", "shown"].into_iter().collect());
    }
    #[test] fn denied_names_are_masked_in_every_path_form() {
        let home = ["/ho", "me/x"].concat();
        for w in [format!("({home}/.ssh/id_rsa)"), format!("\"{home}/.ssh/id_rsa\""), format!("'{home}/.ssh/id_rsa'"), "foo/.ssh/key".into(), "./.env".into(), ".env".into(),
                  "$HOME/.aws/credentials".into(), format!("C:\\{}\\x\\.ssh\\id_rsa", ["Us", "ers"].concat()), "~/.ssh/config.".into(), "[src/.env.local]".into(), "x=/srv/p/.git-credentials".into()] {
            assert!(path_word_denied(&w, &deny()), "{w}");
            let l = label_of(&format!("read {w} now"), &deny());
            assert_eq!(l, "read [hidden] now", "{w}");
        }
        let win_docs = format!("C:\\{}\\x\\docs", ["Us", "ers"].concat());
        for w in ["src/main.rs", "./run.sh", "$PATH", "plain", "see (notes)", win_docs.as_str()] { assert!(!path_word_denied(w, &deny()), "{w}"); }
    }
    #[test] fn path_like_words_naming_a_denied_name_are_masked_in_the_label() {
        let a = ev(r#"{"t":5,"s":"a","c":"/p","k":"start","m":"look at /srv/work/x and ~/.ssh/id_rsa then /srv/ok/file"}"#);
        assert_eq!(a.label, "look at [hidden] and [hidden] then /srv/ok/file");
        let b = ev(r#"{"t":5,"s":"a","c":"/p","k":"start","m":"the work item and a build step"}"#);
        assert_eq!(b.label, "the work item and a build step", "plain words are not paths");
    }
    #[test] fn a_whole_last_line_without_a_newline_is_kept_and_an_exact_boundary_cut_keeps_the_first_line() {
        let d = tempfile::tempdir().unwrap(); let p = d.path().join("n.jsonl");
        std::fs::write(&p, format!("{}\n{}\n{}", start(10, "a", "one"), end(20, "a"), start(30, "a", "last"))).unwrap();
        let t = read_recent(&p, &deny(), None, 10, 40);
        assert_eq!(t.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(), vec!["last", "one"]);
        // a file whose tail window begins exactly after a newline: the first line is whole
        let l1 = format!("{}\n", start(100, "b", "first-whole")); let l2 = format!("{}\n", end(110, "b"));
        let prefix = format!("{}\n", "x".repeat(99)); // 100 bytes, so the tail window starts right after this newline
        let pad = TAIL_BYTES as usize - l1.len() - l2.len();
        let s = format!("{prefix}{l1}{}\n{l2}", "y".repeat(pad - 1));
        assert_eq!(s.len() as u64, TAIL_BYTES + 100);
        let q = d.path().join("e.jsonl"); std::fs::write(&q, &s).unwrap();
        let t = read_recent(&q, &deny(), Some("b"), 10, 200);
        assert_eq!((t.len(), t[0].duration), (1, Some(10)));
    }

    // ---- tokens ----
    fn tline(ts: &str, sid: &str, id: &str, i: u64, o: u64, cc: u64, cr: u64) -> String {
        format!(r#"{{"type":"assistant","timestamp":"{ts}","sessionId":"{sid}","message":{{"id":"{id}","role":"assistant","content":[{{"type":"text","text":"CONTENT-MARKER"}}],"usage":{{"input_tokens":{i},"output_tokens":{o},"cache_creation_input_tokens":{cc},"cache_read_input_tokens":{cr}}}}}}}"#)
    }
    fn setup(lines: &[String]) -> (tempfile::TempDir, String) {
        let d = tempfile::tempdir().unwrap(); let dir = d.path().join("projects/-srv-proj"); std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("sess-1.jsonl"); std::fs::write(&f, lines.join("\n") + "\n").unwrap();
        let tp = std::fs::canonicalize(&f).unwrap().to_string_lossy().into_owned(); (d, tp)
    }
    fn task(tp: Option<&str>, start: i64, end: Option<i64>) -> Task {
        Task { id: "x".into(), session: "sess-1".into(), project: String::new(), start, end, duration: end.map(|e| e - start), state: "done".into(), label: String::new(), tokens: None, tokens_why: String::new(), tp: tp.map(str::to_string) }
    }
    #[test] fn timestamps_parse_to_epoch_seconds() {
        assert_eq!(parse_ts("2026-11-11T07:50:39.626Z"), Some(1794383439));
        assert_eq!(parse_ts("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_ts("2024-02-29T23:59:59Z"), Some(1709251199));
        for bad in ["", "2026-11-11", "2026-13-11T00:00:00Z", "2026-11-11T07:50:39+01:00", "garbage-garbage-garbage"] { assert_eq!(parse_ts(bad), None, "{bad}"); }
    }
    #[test] fn sums_the_four_usage_numbers_in_the_window_and_keeps_cache_reads_apart() {
        let base = parse_ts("2026-11-11T10:00:00Z").unwrap();
        let (d, tp) = setup(&[
            tline("2026-11-11T09:59:59.900Z", "sess-1", "m0", 1, 1, 1, 1),   // one second before the window
            tline("2026-11-11T10:00:00.000Z", "sess-1", "m1", 10, 20, 30, 400), // on the start boundary
            tline("2026-11-11T10:00:30.500Z", "sess-1", "m2", 1, 2, 3, 4),
            tline("2026-11-11T10:00:30.700Z", "other", "m3", 1000, 1000, 1000, 1000), // another session's line
            tline("2026-11-11T10:01:00.999Z", "sess-1", "m4", 5, 5, 5, 5),   // on the end boundary (same second)
            tline("2026-11-11T10:01:01.000Z", "sess-1", "m5", 7, 7, 7, 7),   // after the window
        ]);
        let mut rows = vec![task(Some(&tp), base, Some(base + 60))];
        fill_tokens(&mut rows, Some(d.path()));
        assert_eq!(rows[0].tokens, Some(Tokens { input: 16, output: 27, cache_creation: 38, cache_read: 409 }), "{}", rows[0].tokens_why);
        assert!(rows[0].tokens_why.is_empty());
        let j = serde_json::to_string(&rows).unwrap();
        assert!(!j.contains("CONTENT-MARKER") && !j.contains("sess-1.jsonl") && !j.contains("projects"), "{j}");
    }
    #[test] fn repeated_message_ids_count_once_malformed_lines_are_skipped_and_content_is_never_read() {
        let mut lines = vec![tline("2026-11-11T10:00:01Z", "sess-1", "m1", 1, 5, 0, 100), tline("2026-11-11T10:00:02Z", "sess-1", "m1", 1, 9, 0, 100)];
        lines.push("not json at all".into());
        lines.push(r#"{"timestamp":"2026-11-11T10:00:03Z","message":{"usage":{"input_tokens":"text"}}}"#.into()); // wrong type: skipped
        lines.push(r#"{"timestamp":"2026-11-11T10:00:04Z","message":{"content":"{\"usage\":{\"input_tokens\":999}}"}}"#.into()); // numbers inside content are not usage
        lines.push(r#"{"timestamp":"2026-11-11T10:00:05Z","message":{"content":[{"type":"tool_use","input":{"usage":{"input_tokens":888}}}]}}"#.into());
        lines.push(r#"{"type":"user","timestamp":"2026-11-11T10:00:06Z","message":{"role":"user","content":"hello"}}"#.into());
        let (l, skipped) = load_usage((lines.join("\n") + "\n").as_bytes(), false);
        assert_eq!(skipped, 2);
        let base = parse_ts("2026-11-11T10:00:00Z").unwrap();
        assert_eq!(sum_window(&l, "sess-1", base, base + 100), Tokens { input: 1, output: 9, cache_creation: 0, cache_read: 100 });
    }
    #[test] fn n_a_reasons_for_missing_hook_field_missing_file_and_running_tasks() {
        let (d, tp) = setup(&[tline("2026-11-11T10:00:01Z", "sess-1", "m1", 1, 1, 1, 1)]);
        let mut rows = vec![task(None, 1, Some(2)), task(Some(&format!("{tp}.missing.jsonl")), 1, Some(2)), task(Some(&tp), 1, None)];
        fill_tokens(&mut rows, Some(d.path()));
        assert!(rows[0].tokens.is_none() && rows[0].tokens_why.contains("did not log a transcript path"));
        assert!(rows[1].tokens.is_none() && rows[1].tokens_why.contains("missing"));
        assert!(rows[2].tokens.is_none() && rows[2].tokens_why.is_empty(), "a running task shows nothing");
    }
    #[test] fn refuses_symlinks_relative_wrong_extension_and_out_of_root_paths() {
        let (d, tp) = setup(&[tline("2026-11-11T10:00:01Z", "sess-1", "m1", 1, 1, 1, 1)]);
        let root = d.path();
        assert!(open_transcript(root, &tp).is_ok());
        // symlinked file inside the projects folder
        let link = root.join("projects/-srv-proj/link.jsonl"); std::os::unix::fs::symlink(&tp, &link).unwrap();
        assert!(open_transcript(root, &link.to_string_lossy()).is_err());
        // symlinked folder inside the projects folder
        let dl = root.join("projects/-srv-linked"); std::os::unix::fs::symlink(root.join("projects/-srv-proj"), &dl).unwrap();
        assert!(open_transcript(root, &dl.join("sess-1.jsonl").to_string_lossy()).is_err());
        // outside the root, even a plain file
        let out = tempfile::tempdir().unwrap(); let of = out.path().join("o.jsonl"); std::fs::write(&of, "{}\n").unwrap();
        assert!(open_transcript(root, &of.to_string_lossy()).is_err());
        // a symlink inside the root pointing outside
        let esc = root.join("projects/-srv-proj/esc.jsonl"); std::os::unix::fs::symlink(&of, &esc).unwrap();
        assert!(open_transcript(root, &esc.to_string_lossy()).is_err());
        // not absolute, wrong extension, dot-dot, too shallow
        assert!(open_transcript(root, "projects/-srv-proj/sess-1.jsonl").is_err());
        assert!(open_transcript(root, &tp.replace(".jsonl", ".txt")).is_err());
        assert!(open_transcript(root, &format!("{}/projects/-srv-proj/../-srv-proj/sess-1.jsonl", root.display())).is_err());
        std::fs::write(root.join("projects/top.jsonl"), "{}\n").unwrap();
        assert!(open_transcript(root, &root.join("projects/top.jsonl").to_string_lossy()).is_err());
        // a folder named like a transcript is not a regular file
        std::fs::create_dir_all(root.join("projects/-srv-proj/dir.jsonl")).unwrap();
        assert!(open_transcript(root, &root.join("projects/-srv-proj/dir.jsonl").to_string_lossy()).is_err());
        let mut rows = vec![task(Some(&esc.to_string_lossy()), 1, Some(2))];
        fill_tokens(&mut rows, Some(root));
        assert!(rows[0].tokens.is_none() && !rows[0].tokens_why.contains(&root.to_string_lossy().to_string()), "the reason never carries a path");
    }
    #[test] fn a_transcript_over_the_cap_is_read_from_its_end_and_is_n_a_when_the_task_starts_earlier() {
        let d = tempfile::tempdir().unwrap(); let dir = d.path().join("projects/-srv-proj"); std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("big.jsonl");
        let mut s = String::new();
        let old = tline("2026-11-11T08:00:00Z", "sess-1", "old", 1, 1, 1, 1);
        while (s.len() as u64) < TRANSCRIPT_CAP + 10_000 { s += &old; s.push('\n'); }
        s += &tline("2026-11-11T11:00:00Z", "sess-1", "new", 3, 4, 5, 6); s.push('\n');
        std::fs::write(&f, &s).unwrap();
        let tp = std::fs::canonicalize(&f).unwrap().to_string_lossy().into_owned();
        let late = parse_ts("2026-11-11T10:59:00Z").unwrap(); let early = parse_ts("2026-11-11T07:00:00Z").unwrap();
        let mut rows = vec![task(Some(&tp), late, Some(late + 120)), task(Some(&tp), early, Some(early + 7200))];
        fill_tokens(&mut rows, Some(d.path()));
        // the tail starts with old lines (08:00), so a task starting at 10:59 is covered; one starting at 07:00 is not
        assert_eq!(rows[0].tokens, Some(Tokens { input: 3, output: 4, cache_creation: 5, cache_read: 6 }));
        assert!(rows[1].tokens.is_none() && rows[1].tokens_why.contains("over 32 MB"), "{}", rows[1].tokens_why);
    }
    #[test] fn the_hook_snippet_and_events_carry_the_transcript_path_when_present() {
        let a = ev(r#"{"t":5,"s":"a","c":"/p","tp":"/x/projects/p/a.jsonl","k":"start","m":"x"}"#); assert_eq!(a.tp.as_deref(), Some("/x/projects/p/a.jsonl"));
        let b = ev(r#"{"t":5,"s":"a","c":"/p","tp":null,"k":"end"}"#); assert_eq!(b.tp, None);
        let t = pair(&[a, ev(r#"{"t":9,"s":"a","c":"/p","tp":"/x/projects/p/b.jsonl","k":"end"}"#)], 10);
        assert_eq!(t[0].tp.as_deref(), Some("/x/projects/p/b.jsonl"), "the end line's path wins");
        assert!(!serde_json::to_string(&t).unwrap().contains("projects/p"), "the path is never serialised");
    }
}
