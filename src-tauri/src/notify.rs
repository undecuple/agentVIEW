//! Webhook notifications: "<agent> may be waiting for you". Up to five saved https addresses, each with its own events, sent with a fixed `curl` command.
//! The address is a secret: it never goes back to the page (only its host name), is never in a command line (it is passed
//! on curl's standard input), and is not part of bundles. The signal is only what the app observes (a running session quiet for
//! 10 minutes, or a session that ended with an error); it never claims that an approval is pending.
use agent_app_sessions::Activity;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const IDLE_MS: u64 = 10 * 60 * 1000;
const POLL: Duration = Duration::from_secs(30);
const MAX_PER_HOUR: usize = 20;
static LAST: Mutex<String> = Mutex::new(String::new());
static SENT: Mutex<Vec<(i64, Instant)>> = Mutex::new(Vec::new());

#[derive(Serialize)]
pub struct HookRow { id: i64, name: String, host: Option<String>, enabled: bool, events: Vec<String> }
#[derive(Serialize)]
pub struct WebhookStatus { hooks: Vec<HookRow>, last: String, events: Vec<&'static str> }

fn program() -> std::ffi::OsString {
    #[cfg(feature = "workspace-e2e")]
    if let Some(p) = std::env::var_os("AGENT_APP_TEST_CURL") { return p; }
    "curl".into()
}
fn set_last(s: String) { if let Ok(mut l) = LAST.lock() { *l = s; } }

/// Send one message. The address travels on stdin as a curl config line; arguments hold only fixed flags and the message body.
fn send(id: i64, url: &str, text: &str) -> Result<(), String> {
    agent_app_workspace::webhook::validate_url(url)?;
    {
        let mut s = SENT.lock().map_err(|_| "Send state unavailable")?;
        s.retain(|(_, t)| t.elapsed() < Duration::from_secs(3600));
        if s.iter().filter(|(i, _)| *i == id).count() >= MAX_PER_HOUR { return Err("Hourly limit reached (20 messages for this destination); this one was not sent".into()); }
        s.push((id, Instant::now()));
    }
    let body = serde_json::json!({"content": text, "text": text}).to_string();
    let mut c = Command::new(program());
    c.args(["-q", "--silent", "--show-error", "--proto", "=https", "--proto-redir", "=https", "--max-redirs", "0", "--max-time", "10", "--connect-timeout", "5",
        "--output", "/dev/null", "--write-out", "%{http_code}", "--header", "Content-Type: application/json", "--data-binary"])
        .arg(&body).args(["-K", "-"])
        .env_clear().env("PATH", std::env::var_os("PATH").unwrap_or_default()).env("LC_ALL", "C")
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    for k in ["SSL_CERT_FILE", "SSL_CERT_DIR", "HTTPS_PROXY", "https_proxy", "NO_PROXY", "no_proxy"] { if let Some(v) = std::env::var_os(k) { c.env(k, v); } }
    let mut child = c.spawn().map_err(|_| "curl is not available")?;
    child.stdin.take().ok_or("curl stdin unavailable")?.write_all(format!("url = \"{}\"\n", url.trim()).as_bytes()).map_err(|_| "Could not hand the address to curl")?;
    let start = Instant::now();
    let st = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() < Duration::from_secs(15) => std::thread::sleep(Duration::from_millis(20)),
            _ => { let _ = child.kill(); let _ = child.wait(); return Err("The webhook did not answer in time".into()); }
        }
    };
    let mut code = String::new();
    if let Some(mut o) = child.stdout.take() { use std::io::Read; let _ = (&mut o).take(16).read_to_string(&mut code); }
    if st.success() && code.starts_with('2') { Ok(()) } else { Err(format!("The webhook was not accepted (curl exit {}, HTTP {})", st.code().unwrap_or(-1), code.trim())) }
}

fn clean_name(s: &str) -> String { s.chars().filter(|c| !c.is_control()).take(80).collect() }

#[derive(Default, Clone, Copy)]
pub struct Seen { running: bool, idle_sent: bool }

/// Pure decision: which messages should go out now. Only changes are reported, once each.
/// Each message comes with its event kind: "quiet", "error" or "finished".
pub fn decide(seen: &mut HashMap<String, Seen>, acts: &[Activity]) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    let mut next = HashMap::new();
    for a in acts {
        let prev = seen.get(&a.id).copied();
        let mut cur = Seen { running: a.running, idle_sent: prev.is_some_and(|p| p.idle_sent && a.running) };
        let name = clean_name(&a.display_name);
        let prov = if matches!(a.provider, agent_app_sessions::Provider::Claude) { "Claude Code" } else { "Codex" };
        if a.running {
            match a.idle_ms {
                Some(ms) if ms > IDLE_MS && !cur.idle_sent => { out.push(("quiet", format!("agentVIEW: {name} ({prov}) may be waiting for you. No output for {} min.", ms / 60000))); cur.idle_sent = true; }
                Some(ms) if ms <= IDLE_MS => cur.idle_sent = false,
                _ => {}
            }
        } else if prev.is_some_and(|p| p.running) && a.status == "exited" {
            match a.exit_info.as_deref() {
                Some("exit 0") => out.push(("finished", format!("agentVIEW: {name} ({prov}) finished (exit 0)."))),
                Some(info) => out.push(("error", format!("agentVIEW: {name} ({prov}) stopped with {}.", clean_name(info)))),
                None => {}
            }
        }
        next.insert(a.id.clone(), cur);
    }
    *seen = next;
    out
}

pub fn start_monitor(app: tauri::AppHandle) {
    use tauri::Manager;
    std::thread::spawn(move || {
        let mut seen: HashMap<String, Seen> = HashMap::new();
        loop {
            std::thread::sleep(POLL);
            let live = app.state::<super::live::LiveState>();
            let Some(acts) = live.peek_activity() else { continue };
            let msgs = decide(&mut seen, &acts);
            if msgs.is_empty() { continue; }
            let state = app.state::<super::WorkspaceState>();
            let hooks = state.0.as_ref().ok().and_then(|m| m.lock().ok()).and_then(|s| s.webhooks().ok()).unwrap_or_default();
            for (kind, m) in msgs.into_iter().take(5) {
                for h in hooks.iter().filter(|h| h.enabled && h.events.iter().any(|e| e == kind)) {
                    match send(h.id, &h.url, &m) { Ok(()) => set_last(format!("Last message sent to {}", h.name)), Err(e) => set_last(format!("Last send to {} failed: {e}", h.name)) }
                    std::thread::sleep(Duration::from_secs(2));
                }
            }
        }
    });
}

fn status_of(s: &mut agent_app_workspace::Store) -> Result<WebhookStatus, String> {
    let last = LAST.lock().map(|l| l.clone()).unwrap_or_default();
    let hooks = s.webhooks()?.into_iter().map(|h| HookRow { host: agent_app_workspace::webhook::validate_url(&h.url).ok(), id: h.id, name: h.name, enabled: h.enabled, events: h.events }).collect();
    Ok(WebhookStatus { hooks, last, events: agent_app_workspace::webhook::EVENTS.to_vec() })
}

#[tauri::command]
pub fn webhook_status(window: tauri::WebviewWindow, state: tauri::State<super::WorkspaceState>) -> Result<WebhookStatus, String> { super::with_store(&window, &state, status_of) }
/// Add (id null, address required) or edit (address optional: left empty keeps the saved one) a destination.
#[tauri::command]
pub fn webhook_save(window: tauri::WebviewWindow, state: tauri::State<super::WorkspaceState>, id: Option<i64>, name: String, url: Option<String>, enabled: bool, events: Vec<String>) -> Result<WebhookStatus, String> {
    super::with_store(&window, &state, |s| {
        let u = url.as_deref().map(str::trim).filter(|u| !u.is_empty());
        s.webhook_save(id, &name, u, enabled, &events)?;
        status_of(s)
    })
}
#[tauri::command]
pub fn webhook_clear(window: tauri::WebviewWindow, state: tauri::State<super::WorkspaceState>, id: i64) -> Result<WebhookStatus, String> {
    super::with_store(&window, &state, |s| { s.webhook_remove(id)?; status_of(s) })
}
/// Sends one fixed test message to a saved destination; the page supplies no text and no address.
#[tauri::command]
pub async fn webhook_test(window: tauri::WebviewWindow, state: tauri::State<'_, super::WorkspaceState>, id: i64) -> Result<String, String> {
    let hook = super::with_store(&window, &state, |s| Ok(s.webhooks()?.into_iter().find(|h| h.id == id)))?.ok_or("That destination no longer exists")?;
    let name = hook.name.clone();
    let r = tauri::async_runtime::spawn_blocking(move || send(hook.id, &hook.url, "agentVIEW test message: notifications are working.")).await.map_err(|_| "Test failed")?;
    match &r { Ok(()) => set_last(format!("Test message sent to {name}")), Err(e) => set_last(format!("Test to {name} failed: {e}")) }
    r.map(|()| format!("Test message sent to {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_app_sessions::Provider;
    fn act(id: &str, running: bool, idle: Option<u64>, status: &str, exit: Option<&str>) -> Activity {
        Activity { id: id.into(), display_name: format!("Agent {id}"), label: String::new(), project: String::new(), provider: Provider::Claude, status: status.into(), running,
            output_total: 0, input_total: 0, idle_ms: idle, action: None, action_at: None, exit_info: exit.map(String::from), exit_at: None }
    }
    #[test] fn idle_notifies_once_and_rearms() {
        let mut seen = HashMap::new();
        assert!(decide(&mut seen, &[act("a", true, Some(1000), "running", None)]).is_empty());
        let m = decide(&mut seen, &[act("a", true, Some(11 * 60000), "running", None)]);
        assert_eq!(m.len(), 1); assert!(m[0].0 == "quiet" && m[0].1.contains("Agent a") && m[0].1.contains("may be waiting") && m[0].1.contains("11 min"));
        assert!(decide(&mut seen, &[act("a", true, Some(12 * 60000), "running", None)]).is_empty());
        assert!(decide(&mut seen, &[act("a", true, Some(500), "running", None)]).is_empty());
        assert_eq!(decide(&mut seen, &[act("a", true, Some(11 * 60000), "running", None)]).len(), 1);
    }
    #[test] fn error_and_clean_exits_have_their_own_events_and_old_sessions_are_ignored() {
        let mut seen = HashMap::new();
        decide(&mut seen, &[act("a", true, Some(0), "running", None), act("b", true, Some(0), "running", None)]);
        let m = decide(&mut seen, &[act("a", false, None, "exited", Some("exit 1")), act("b", false, None, "exited", Some("exit 0")), act("c", false, None, "exited", Some("exit 2"))]);
        assert_eq!(m.len(), 2); assert!(m.iter().any(|(k, s)| *k == "error" && s.contains("Agent a") && s.contains("exit 1")));
        assert!(m.iter().any(|(k, s)| *k == "finished" && s.contains("Agent b")));
        assert!(decide(&mut seen, &[act("a", false, None, "exited", Some("exit 1"))]).is_empty());
    }
    #[test] fn names_are_cleaned() { assert_eq!(clean_name("a\nb\u{7}c"), "abc"); assert_eq!(clean_name(&"x".repeat(200)).len(), 80); }
}
