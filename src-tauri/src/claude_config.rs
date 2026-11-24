//! Read-only, redacted view of Claude Code's global state file (~/.claude.json). Shows structure only: project
//! folders, MCP server names with a command or host name, and trust flags. It never returns env values, arguments,
//! headers, tokens, account details or conversation history, and it never writes the file (Claude Code rewrites it while running).
use serde::Serialize;
use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;

const CAP: u64 = 16 * 1024 * 1024;

#[derive(Serialize)]
pub struct McpServer { name: String, kind: String, target: String, has_env: bool }
#[derive(Serialize)]
pub struct ProjectEntry { path: String, trusted: Option<bool>, allowed_tools: usize, history_entries: usize, mcp: Vec<McpServer>, exists: bool }
#[derive(Serialize)]
pub struct ConfigView { found: bool, size: u64, modified: Option<String>, top_level_keys: usize, has_account_data: bool, global_mcp: Vec<McpServer>, projects: Vec<ProjectEntry>, note: &'static str }

fn file() -> Option<PathBuf> {
    #[cfg(feature = "workspace-e2e")]
    if let Some(p) = std::env::var_os("AGENT_APP_TEST_CLAUDE_JSON") { return Some(PathBuf::from(p)); }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude.json"))
}
/// Command name only: the first word, then its file name. Inline arguments or KEY=value text never show.
pub(crate) fn base(s: &str) -> String {
    let first = s.split_whitespace().next().unwrap_or("");
    if first.contains('=') { return String::new(); } // a KEY=value word is never shown
    first.rsplit('/').next().unwrap_or("").chars().take(60).collect()
}
/// Host name only: no scheme, user info, port-less path, query or fragment.
fn host(u: &str) -> String {
    let rest = u.split_once("://").map_or(u, |x| x.1);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    authority.rsplit('@').next().unwrap_or("").chars().take(80).collect()
}
fn servers(v: Option<&Value>) -> Vec<McpServer> {
    let Some(m) = v.and_then(Value::as_object) else { return vec![] };
    let mut out: Vec<McpServer> = m.iter().take(200).map(|(name, s)| {
        let kind = s.get("type").and_then(Value::as_str).unwrap_or("stdio").chars().take(12).collect::<String>();
        let target = match (s.get("command").and_then(Value::as_str), s.get("url").and_then(Value::as_str)) {
            (Some(c), _) => base(c), (_, Some(u)) => host(u), _ => String::new() };
        let has_env = s.get("env").and_then(Value::as_object).is_some_and(|e| !e.is_empty()) || s.get("headers").and_then(Value::as_object).is_some_and(|e| !e.is_empty());
        McpServer { name: name.chars().take(60).collect(), kind, target, has_env }
    }).collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[tauri::command]
pub fn claude_config_view(window: tauri::WebviewWindow) -> Result<ConfigView, String> {
    super::authorized(&window)?;
    const NOTE: &str = "Read-only. Values such as environment variables, arguments, headers, tokens and account details are never shown.";
    let Some(path) = file() else { return Err("Home folder unknown".into()) };
    let meta = match std::fs::symlink_metadata(&path) { Ok(m) => m, Err(_) => return Ok(ConfigView { found: false, size: 0, modified: None, top_level_keys: 0, has_account_data: false, global_mcp: vec![], projects: vec![], note: NOTE }) };
    if !meta.is_file() || meta.len() > CAP { return Err("~/.claude.json is not a plain file under 16 MB".into()); }
    let mut text = String::new();
    std::fs::File::open(&path).and_then(|f| f.take(CAP).read_to_string(&mut text)).map_err(|_| "Cannot read ~/.claude.json")?;
    let v: Value = serde_json::from_str(&text).map_err(|_| "~/.claude.json is not valid JSON (Claude Code may be writing it; try again)")?;
    let obj = v.as_object().ok_or("~/.claude.json has an unexpected shape")?;
    let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs().to_string());
    let mut projects: Vec<ProjectEntry> = obj.get("projects").and_then(Value::as_object).map(|m| m.iter().take(500).map(|(p, e)| ProjectEntry {
        path: p.chars().take(300).collect(),
        trusted: e.get("hasTrustDialogAccepted").and_then(Value::as_bool),
        allowed_tools: e.get("allowedTools").and_then(Value::as_array).map_or(0, Vec::len),
        history_entries: e.get("history").and_then(Value::as_array).map_or(0, Vec::len),
        mcp: servers(e.get("mcpServers")),
        exists: std::path::Path::new(p).is_dir(),
    }).collect()).unwrap_or_default();
    projects.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ConfigView { found: true, size: meta.len(), modified, top_level_keys: obj.len(), has_account_data: obj.contains_key("oauthAccount") || obj.contains_key("userID"),
        global_mcp: servers(obj.get("mcpServers")), projects, note: NOTE })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn host_never_shows_path_query_or_user_info() {
        assert_eq!(host("https://h.example/mcp?key=SECRET"), "h.example");
        assert_eq!(host("https://h.example?key=SECRET"), "h.example");
        let with_user = format!("https://user:pw{}h.example:8443/a#frag", '@');
        assert_eq!(host(&with_user), "h.example:8443");
        assert_eq!(host("h.example/x"), "h.example");
    }
    #[test] fn base_never_shows_inline_arguments() {
        assert_eq!(base("/usr/bin/tool"), "tool");
        assert_eq!(base("tool --token=SECRET KEY=VALUE"), "tool");
        assert_eq!(base("KEY=SECRET tool"), "");
    }
}
