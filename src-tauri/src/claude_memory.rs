//! Read-only views of Claude Code's memory and instruction files, and of its past-session list.
//! Memory: `projects/<slug>/memory/*.md` and `CLAUDE.md` in the Claude config folder, plus `CLAUDE.md` / `AGENTS.md` in
//! allow-listed folders (the path allow-list and deny names apply). History: file names, sizes and times of the session
//! files only; session contents (the conversation) are never opened. Nothing is written or run.
use super::claude_settings::{ok_name, read_head, root};
use agent_app_core::{policy, Config};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;

const READ_CAP: u64 = 64 * 1024;

#[derive(Serialize, Debug, PartialEq)]
pub struct MemEntry { key: String, group: String, name: String, size: u64, modified: u64 }
#[derive(Serialize)]
pub struct MemFile { name: String, text: String, truncated: bool }
#[derive(Serialize, Debug, PartialEq)]
pub struct SessionRow { project: String, id: String, size: u64, modified: u64, resume: String }

fn slug_ok(s: &str) -> bool { !s.is_empty() && s.len() <= 200 && !s.starts_with('.') && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')) }
fn secs(m: &std::fs::Metadata) -> u64 { m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs()) }

/// Resolve a key from the list to a path. Only shapes this module produced are accepted.
fn resolve(key: &str, cfg: &Config) -> Option<(PathBuf, String)> {
    let r = root()?;
    let parts: Vec<&str> = key.split('|').collect();
    let (path, name) = match parts.as_slice() {
        ["g", f] if *f == "CLAUDE.md" => (r.join("CLAUDE.md"), "CLAUDE.md".to_string()),
        ["m", slug, f] if slug_ok(slug) && ok_name(f.strip_suffix(".md")?) => (r.join("projects").join(slug).join("memory").join(f), format!("{f}")),
        ["a", idx, f] if matches!(*f, "CLAUDE.md" | "AGENTS.md") => (cfg.allow.get(idx.parse::<usize>().ok()?)?.join(f), format!("{f}")),
        _ => return None,
    };
    let real = std::fs::canonicalize(&path).ok()?;
    if !real.is_file() || super::gitview::denied_name(&real.to_string_lossy(), cfg) { return None; }
    Some((path, name))
}

#[tauri::command]
pub fn claude_memory_list(window: tauri::WebviewWindow, cfg: tauri::State<Arc<Config>>) -> Result<Vec<MemEntry>, String> {
    super::authorized(&window)?;
    let cfg = super::docshare::viewer_config(&cfg); // allowlist.toml plus the folders shared on the Documents page
    let r = root().ok_or("Home folder unknown")?;
    let mut out: Vec<MemEntry> = Vec::new();
    let mut add = |key: String, group: String, name: String, path: &std::path::Path| {
        if let Ok(m) = std::fs::metadata(path) { if m.is_file() && out.len() < 600 { out.push(MemEntry { key, group, name, size: m.len(), modified: secs(&m) }); } }
    };
    add("g|CLAUDE.md".into(), "Global".into(), "CLAUDE.md".into(), &r.join("CLAUDE.md"));
    for (i, a) in cfg.allow.iter().enumerate().take(50) {
        for f in ["CLAUDE.md", "AGENTS.md"] { let p = a.join(f); add(format!("a|{i}|{f}"), format!("Allowed folder {}", policy::short(&a.to_string_lossy())), f.into(), &p); }
    }
    if let Ok(rd) = std::fs::read_dir(r.join("projects")) {
        let mut slugs: Vec<_> = rd.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|s| slug_ok(s)).take(200).collect();
        slugs.sort();
        for s in slugs {
            let Ok(rd) = std::fs::read_dir(r.join("projects").join(&s).join("memory")) else { continue };
            let mut files: Vec<String> = rd.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|f| f.strip_suffix(".md").is_some_and(ok_name)).take(200).collect();
            files.sort();
            for f in files { let p = r.join("projects").join(&s).join("memory").join(&f); add(format!("m|{s}|{f}"), format!("Memory: {s}"), f, &p); }
        }
    }
    out.retain(|e| resolve(&e.key, &cfg).is_some());
    Ok(out)
}

#[tauri::command]
pub fn claude_memory_read(window: tauri::WebviewWindow, cfg: tauri::State<Arc<Config>>, key: String) -> Result<MemFile, String> {
    super::authorized(&window)?;
    let cfg = super::docshare::viewer_config(&cfg); // allowlist.toml plus the folders shared on the Documents page
    let (path, name) = resolve(&key, &cfg).ok_or("That file is not available")?;
    let len = std::fs::metadata(&path).map_err(|_| "Unreadable")?.len();
    let text = read_head(&path, READ_CAP).ok_or("Unreadable")?;
    let clean: String = text.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).collect();
    Ok(MemFile { name, text: policy::redact(&clean), truncated: len > READ_CAP })
}

#[tauri::command]
pub fn claude_sessions_list(window: tauri::WebviewWindow) -> Result<Vec<SessionRow>, String> {
    super::authorized(&window)?;
    let r = root().ok_or("Home folder unknown")?;
    let mut rows: Vec<SessionRow> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(r.join("projects")) {
        for e in rd.flatten().take(300) {
            let Ok(slug) = e.file_name().into_string() else { continue };
            if !slug_ok(&slug) { continue; }
            let Ok(files) = std::fs::read_dir(e.path()) else { continue };
            for f in files.flatten().take(2000) {
                let Ok(fname) = f.file_name().into_string() else { continue };
                let Some(id) = fname.strip_suffix(".jsonl").filter(|i| ok_name(i) && i.len() <= 64) else { continue };
                let Ok(m) = std::fs::symlink_metadata(f.path()) else { continue };
                if !m.is_file() { continue; }
                rows.push(SessionRow { project: slug.clone(), id: id.into(), size: m.len(), modified: secs(&m), resume: format!("claude --resume {id}") });
            }
        }
    }
    rows.sort_by(|a, b| b.modified.cmp(&a.modified));
    rows.truncate(300);
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn slugs_and_keys() {
        assert!(slug_ok("-home-x-proj") && !slug_ok("../x") && !slug_ok(".hidden") && !slug_ok("a/b") && !slug_ok(""));
        let cfg = Config::default();
        for bad in ["m|../x|a.md", "m|ok|../a.md", "m|ok|a.txt", "g|other.md", "a|99|CLAUDE.md", "a|x|CLAUDE.md", "zz", "m|ok"] { assert!(resolve(bad, &cfg).is_none(), "{bad}"); }
    }
}
