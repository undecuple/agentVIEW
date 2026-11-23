//! Read-only overview data per registered session: process state, terminal byte counters, idle time and, for
//! Claude Code, the NAME of the latest tool it used (never prompts, arguments or output). Nothing here starts,
//! writes to or signals a process, and any problem with the provider's log simply leaves the action empty.
use super::{sys, Provider, Result, SessionManager, UsageRoots};
use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;

const TAIL_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Activity {
    pub id: String,
    pub display_name: String,
    pub label: String,
    pub project: String,
    pub provider: Provider,
    pub status: String,
    pub running: bool,
    /// Bytes the terminal has produced / received since this launch (monotonic counters; the UI derives rates).
    pub output_total: u64,
    pub input_total: u64,
    pub idle_ms: Option<u64>,
    pub action: Option<String>,
    pub action_at: Option<String>,
}

/// Latest tool name in the tail of a Claude Code session log. Names are limited to a short safe alphabet.
pub(crate) fn latest_tool(tail: &str) -> Option<(String, Option<String>)> {
    for line in tail.lines().rev() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let Some(items) = value.pointer("/message/content").and_then(|c| c.as_array()) else { continue };
        for item in items.iter().rev() {
            if item.get("type").and_then(|v| v.as_str()) != Some("tool_use") { continue; }
            let Some(name) = item.get("name").and_then(|v| v.as_str()) else { continue };
            if name.is_empty() || name.len() > 48 || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':' | '.')) { continue; }
            let at = value.get("timestamp").and_then(|v| v.as_str()).filter(|s| s.len() <= 40 && s.chars().all(|c| c.is_ascii_digit() || matches!(c, '-' | ':' | 'T' | 'Z' | '.' | '+'))).map(String::from);
            return Some((name.to_string(), at));
        }
    }
    None
}

impl SessionManager {
    pub fn activity(&mut self, roots: &UsageRoots) -> Result<Vec<Activity>> {
        self.refresh()?;
        let roster = self.list()?;
        let mut out = Vec::with_capacity(roster.len());
        for s in roster {
            let (running, output_total, input_total, idle_ms) = match self.running.get(&s.id) {
                Some(run) => {
                    let (end, idle) = run.ring.lock().map(|r| (r.start + r.bytes.len() as u64, r.last_output.map(|t| t.elapsed().as_millis() as u64))).unwrap_or((0, None));
                    (run.child.is_some(), end, run.input_bytes, idle)
                }
                None => (false, 0, 0, None),
            };
            let (action, action_at) = if s.provider == Provider::Claude && running { self.claude_action(&s, roots).unzip() } else { (None, None) };
            out.push(Activity {
                project: std::path::Path::new(&s.project_dir).file_name().and_then(|n| n.to_str()).unwrap_or("").chars().filter(|c| !c.is_control()).take(60).collect(),
                id: s.id, display_name: s.display_name, label: s.label, provider: s.provider, status: s.status, running,
                output_total, input_total, idle_ms, action, action_at: action_at.flatten(),
            });
        }
        Ok(out)
    }

    fn claude_action(&self, s: &super::SessionInfo, roots: &UsageRoots) -> Option<(String, Option<String>)> {
        let (root, relative) = self.usage_location(s, roots).ok()?;
        let mut file = sys::usage_file(&root, &relative).ok()?;
        let meta = file.metadata().ok()?;
        if !meta.is_file() || meta.uid() != sys::uid() || meta.mode() & 0o022 != 0 || meta.nlink() != 1 { return None; }
        let len = meta.len();
        let from = len.saturating_sub(TAIL_BYTES);
        file.seek(SeekFrom::Start(from)).ok()?;
        let mut bytes = Vec::with_capacity((len - from) as usize);
        file.take(TAIL_BYTES).read_to_end(&mut bytes).ok()?;
        let text = String::from_utf8_lossy(&bytes);
        let text = if from > 0 { text.split_once('\n').map(|x| x.1).unwrap_or("") } else { &text };
        latest_tool(text)
    }
}
