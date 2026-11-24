//! Read-only views of Claude Code's own settings and its skills, commands and agents. Fixed locations only
//! (the Claude config folder: settings.json, settings.local.json, skills/, commands/, agents/). Nothing is written.
//! Environment variable VALUES, hook arguments, helper commands and tokens are never returned: env shows names only,
//! hook and status-line commands show the program's file name, permission rules and descriptions pass the token redaction.
use agent_app_core::{policy, Config};
use serde::Serialize;
use serde_json::Value;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const FILE_CAP: u64 = 1024 * 1024;
const HEAD_CAP: u64 = 4096;

pub(crate) fn root() -> Option<PathBuf> {
    #[cfg(feature = "workspace-e2e")]
    if let Some(p) = std::env::var_os("AGENT_APP_TEST_CLAUDE_DIR") { return Some(PathBuf::from(p)); }
    std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude")))
}
fn short(s: &str, n: usize) -> String { policy::redact(&s.chars().filter(|c| !c.is_control()).take(n).collect::<String>()) }

pub(crate) fn read_head(path: &Path, cap: u64) -> Option<String> {
    let f = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(path).ok().or_else(|| {
        // A symlinked entry (for example one managed by home-manager) is allowed when it resolves to a plain file.
        let real = std::fs::canonicalize(path).ok()?; if !real.is_file() { return None; }
        std::fs::OpenOptions::new().read(true).open(real).ok() })?;
    if !f.metadata().ok()?.is_file() { return None; }
    let mut s = String::new(); f.take(cap).read_to_string(&mut s).ok()?; Some(s)
}

// ---------- settings ----------
#[derive(Serialize, Default, Debug, PartialEq)]
pub struct Rule { source: String, text: String }
#[derive(Serialize, Default, Debug, PartialEq)]
pub struct HookRow { source: String, event: String, matcher: String, kind: String, program: String }
#[derive(Serialize, Default, Debug, PartialEq)]
pub struct FileInfo { name: String, found: bool, valid: bool, size: u64 }
#[derive(Serialize, Default, Debug, PartialEq)]
pub struct SettingsView {
    files: Vec<FileInfo>, allow: Vec<Rule>, deny: Vec<Rule>, ask: Vec<Rule>, default_mode: Option<String>, hooks: Vec<HookRow>,
    env_names: Vec<String>, model: Option<String>, plugins: Vec<String>, status_line: Option<String>, helpers_set: Vec<&'static str>, note: &'static str,
}

fn rules(src: &str, v: Option<&Value>, out: &mut Vec<Rule>) {
    for r in v.and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).take(300) { if out.len() < 600 { out.push(Rule { source: src.into(), text: short(r, 200) }); } }
}
/// Folds one settings file into the view. Pure, so it can be tested.
pub fn fold(src: &str, v: &Value, out: &mut SettingsView) {
    let p = v.get("permissions");
    rules(src, p.and_then(|p| p.get("allow")), &mut out.allow);
    rules(src, p.and_then(|p| p.get("deny")), &mut out.deny);
    rules(src, p.and_then(|p| p.get("ask")), &mut out.ask);
    if let Some(m) = p.and_then(|p| p.get("defaultMode")).and_then(Value::as_str) { out.default_mode = Some(short(m, 30)); }
    if let Some(h) = v.get("hooks").and_then(Value::as_object) {
        for (event, groups) in h.iter().take(40) {
            for g in groups.as_array().into_iter().flatten().take(50) {
                let matcher = short(g.get("matcher").and_then(Value::as_str).unwrap_or(""), 60);
                for hk in g.get("hooks").and_then(Value::as_array).into_iter().flatten().take(20) {
                    let kind = short(hk.get("type").and_then(Value::as_str).unwrap_or("command"), 20);
                    let program = hk.get("command").and_then(Value::as_str).map(super::claude_config::base).unwrap_or_default();
                    if out.hooks.len() < 300 { out.hooks.push(HookRow { source: src.into(), event: short(event, 40), matcher: matcher.clone(), kind, program }); }
                }
            }
        }
    }
    if let Some(e) = v.get("env").and_then(Value::as_object) { for k in e.keys().take(200) { let n = short(k, 60); if !out.env_names.contains(&n) { out.env_names.push(n); } } }
    if let Some(m) = v.get("model").and_then(Value::as_str) { out.model = Some(short(m, 60)); }
    if let Some(pl) = v.get("enabledPlugins").and_then(Value::as_object) { for (k, on) in pl.iter().take(100) { if on.as_bool() != Some(false) { let n = short(k, 80); if !out.plugins.contains(&n) { out.plugins.push(n); } } } }
    if let Some(c) = v.get("statusLine").and_then(|s| s.get("command")).and_then(Value::as_str) { out.status_line = Some(super::claude_config::base(c)); }
    if v.get("apiKeyHelper").is_some() && !out.helpers_set.contains(&"apiKeyHelper") { out.helpers_set.push("apiKeyHelper"); }
    if v.get("awsAuthRefresh").is_some() && !out.helpers_set.contains(&"awsAuthRefresh") { out.helpers_set.push("awsAuthRefresh"); }
}

#[tauri::command]
pub fn claude_settings_view(window: tauri::WebviewWindow) -> Result<SettingsView, String> {
    super::authorized(&window)?;
    let mut out = SettingsView { note: "Read-only. Environment values, hook arguments, helper commands and tokens are never shown: env lists names only and hooks show the program name.", ..Default::default() };
    let Some(root) = root() else { return Err("Home folder unknown".into()) };
    for (name, label) in [("settings.json", "user"), ("settings.local.json", "local")] {
        let path = root.join(name);
        let mut info = FileInfo { name: name.into(), ..Default::default() };
        if let Ok(m) = std::fs::symlink_metadata(&path) {
            info.found = true; info.size = m.len();
            if m.len() <= FILE_CAP { if let Some(text) = read_head(&path, FILE_CAP) { if let Ok(v) = serde_json::from_str::<Value>(&text) { info.valid = true; fold(label, &v, &mut out); } } }
        }
        out.files.push(info);
    }
    out.env_names.sort(); out.plugins.sort();
    Ok(out)
}

// ---------- skills, commands, agents ----------
#[derive(Serialize, Debug, PartialEq)]
pub struct Entry { kind: &'static str, name: String, description: String, extra: String }

/// `key: value` lines of a leading `---` block. A block scalar (`>` or `|`) takes its first indented line.
pub fn frontmatter(text: &str) -> Vec<(String, String)> {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") { return vec![]; }
    let mut out: Vec<(String, String)> = Vec::new();
    let mut pending: Option<String> = None;
    for l in lines {
        if l.trim() == "---" { break; }
        if let Some(k) = pending.take() { let v = l.trim(); if !v.is_empty() { out.push((k, v.to_string())); continue; } }
        let Some((k, v)) = l.split_once(':') else { continue };
        if k.starts_with(' ') || k.is_empty() { continue; }
        let v = v.trim().trim_matches(|c| c == '"' || c == '\'');
        if matches!(v, ">" | "|" | ">-" | "|-") { pending = Some(k.trim().to_string()); } else { out.push((k.trim().to_string(), v.to_string())); }
    }
    out
}
fn field(fm: &[(String, String)], key: &str) -> String { fm.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).unwrap_or_default() }
fn first_body_line(text: &str) -> String {
    let body = if text.starts_with("---") { text.splitn(3, "---").nth(2).unwrap_or("") } else { text };
    body.lines().map(|l| l.trim().trim_start_matches('#').trim()).find(|l| !l.is_empty()).unwrap_or("").to_string()
}
pub(crate) fn ok_name(n: &str) -> bool { !n.is_empty() && n.len() <= 80 && n.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':')) }

fn scan(dir: &Path, kind: &'static str, cfg: &Config, out: &mut Vec<Entry>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().take(400).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        if out.len() >= 600 { return; }
        let fname = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        let real = std::fs::canonicalize(&path).unwrap_or(path.clone());
        if super::gitview::denied_name(&real.to_string_lossy(), cfg) { continue; }
        let (name, file) = if kind == "skill" {
            if !real.is_dir() { continue; }
            (fname.clone(), path.join("SKILL.md"))
        } else if real.is_dir() {
            // one level of namespace folders: commands/ns/name.md becomes ns:name
            let Ok(sub) = std::fs::read_dir(&path) else { continue };
            for s in sub.flatten().take(100) {
                let sn = s.file_name().to_string_lossy().into_owned();
                if let Some(stem) = sn.strip_suffix(".md").filter(|st| ok_name(st) && ok_name(&fname)) {
                    if out.len() < 600 { if let Some(t) = read_head(&s.path(), HEAD_CAP) { out.push(entry(kind, format!("{fname}:{stem}"), &t)); } }
                }
            }
            continue;
        } else {
            match fname.strip_suffix(".md") { Some(stem) => (stem.to_string(), path.clone()), None => continue }
        };
        if !ok_name(&name) { continue; }
        if let Some(t) = read_head(&file, HEAD_CAP) { out.push(entry(kind, name, &t)); }
    }
}
fn entry(kind: &'static str, name: String, text: &str) -> Entry {
    let fm = frontmatter(text);
    let d = field(&fm, "description");
    let d = if d.is_empty() { first_body_line(text) } else { d };
    let description = short(&d, 200);
    let extra = if kind == "agent" { [("model", field(&fm, "model")), ("tools", field(&fm, "tools"))].iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| format!("{k}: {}", short(v, 80))).collect::<Vec<_>>().join(" · ") } else { String::new() };
    Entry { kind, name, description, extra }
}

#[tauri::command]
pub fn claude_inventory(window: tauri::WebviewWindow, cfg: tauri::State<Arc<Config>>) -> Result<Vec<Entry>, String> {
    super::authorized(&window)?;
    let root = root().ok_or("Home folder unknown")?;
    let mut out = Vec::new();
    scan(&root.join("skills"), "skill", &cfg, &mut out);
    scan(&root.join("commands"), "command", &cfg, &mut out);
    scan(&root.join("agents"), "agent", &cfg, &mut out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn settings_never_expose_values() {
        let tok = format!("Bearer {}{}", "abcdefghijkl", "mnopqrstuvwxyz012345"); // assembled at run time so no secret-shaped literal sits in the source
        let v = serde_json::json!({"permissions":{"allow":["Bash(git status)", format!("Bash(curl -H 'Authorization: {tok}' x)")],"deny":["Read(./.env)"],"defaultMode":"acceptEdits"},
          "hooks":{"Notification":[{"matcher":"permission_prompt","hooks":[{"type":"command","command":"/usr/bin/notify --token SECRET-ARG"}]}]},
          "env":{"API_TOKEN":"SECRET-VALUE","FOO":"bar"},"model":"opus","apiKeyHelper":"/bin/get-key SECRET-HELPER","statusLine":{"type":"command","command":"bash -c 'echo SECRET-STATUS'"},"enabledPlugins":{"a@b":true,"c@d":false}});
        let mut out = SettingsView::default(); fold("user", &v, &mut out);
        let all = serde_json::to_string(&out).unwrap();
        for s in ["SECRET-VALUE", "SECRET-ARG", "SECRET-HELPER", "SECRET-STATUS", "abcdefghijklmnopqrstuvwxyz012345"] { assert!(!all.contains(s), "{s} leaked"); }
        assert_eq!(out.env_names, vec!["API_TOKEN".to_string(), "FOO".to_string()]);
        assert_eq!((out.hooks[0].program.as_str(), out.hooks[0].matcher.as_str()), ("notify", "permission_prompt"));
        assert_eq!(out.helpers_set, vec!["apiKeyHelper"]); assert_eq!(out.plugins, vec!["a@b".to_string()]); assert_eq!(out.default_mode.as_deref(), Some("acceptEdits"));
        assert_eq!(out.allow.len(), 2); assert_eq!(out.deny.len(), 1);
    }
    #[test] fn frontmatter_and_fallbacks() {
        let t = "---\nname: x\ndescription: >\n  Does a thing well\n  more\nmodel: opus\n---\n# Heading\nBody";
        let fm = frontmatter(t); assert_eq!(field(&fm, "description"), "Does a thing well"); assert_eq!(field(&fm, "model"), "opus");
        assert_eq!(first_body_line("# Title line\nmore"), "Title line"); assert_eq!(first_body_line(t), "Heading");
        assert!(frontmatter("no block").is_empty());
        let e = entry("agent", "a".into(), "---\ndescription: \"Reviews code\"\ntools: Read, Grep\nmodel: sonnet\n---\n");
        assert_eq!((e.description.as_str(), e.extra.as_str()), ("Reviews code", "model: sonnet · tools: Read, Grep"));
    }
    #[test] fn names_are_checked() { assert!(ok_name("my-skill_1") && ok_name("ns:cmd") && !ok_name("../x") && !ok_name("a b") && !ok_name("")); }
}
