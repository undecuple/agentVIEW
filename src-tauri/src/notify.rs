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
fn poll_interval() -> Duration {
    #[cfg(feature = "workspace-e2e")]
    if let Some(ms) = std::env::var("AGENT_APP_TEST_POLL_MS").ok().and_then(|v| v.parse::<u64>().ok()) { return Duration::from_millis(ms.clamp(50, 30_000)); }
    POLL
}
const MAX_PER_HOUR: usize = 20;
static LAST: Mutex<String> = Mutex::new(String::new());
static SENT: Mutex<Vec<(i64, Instant)>> = Mutex::new(Vec::new());
static ATTN: Mutex<Vec<Attn>> = Mutex::new(Vec::new());
static DESK: Mutex<Vec<Instant>> = Mutex::new(Vec::new());
static TL: Mutex<Vec<TlEvent>> = Mutex::new(Vec::new());
const TL_KEEP: usize = 300;
const TL_AGE_S: i64 = 24 * 3600;
const ATTN_KEEP: usize = 30;
const ATTN_AGE_S: i64 = 6 * 3600;
const DESK_PER_HOUR: usize = 30;

/// One thing that may need a person, as the monitor saw it (a quiet session, an error exit, or an agent hook signal). Only a line of text and a time:
/// it is kept in memory, never stored, and never answers or approves anything.
#[derive(Serialize, Clone)]
pub struct Attn { t: i64, kind: &'static str, text: String }
fn now_s() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) }
fn record(kind: &'static str, text: &str) {
    if let Ok(mut a) = ATTN.lock() {
        a.push(Attn { t: now_s(), kind, text: text.chars().filter(|c| !c.is_control()).take(300).collect() });
        let n = a.len(); if n > ATTN_KEEP { a.drain(..n - ATTN_KEEP); }
    }
}
/// One observed change of a session's process state (started, quiet for 10 minutes, active again, ended). Kept in memory for 24 hours; not stored, not exported.
#[derive(Serialize, Clone)]
pub struct TlEvent { t: i64, id: String, name: String, event: String }
/// Pure: the events that follow from one observation. `prev` is (running, quiet) from the last tick. Sessions already there when the monitor first looks (`seeded` false) make no event; later a newly seen running session is "started".
pub fn timeline_events(prev: &mut HashMap<String, (bool, bool)>, acts: &[Activity], seeded: bool) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut next = HashMap::new();
    for a in acts {
        let quiet = a.running && a.idle_ms.is_some_and(|ms| ms > IDLE_MS);
        let name = clean_name(&a.display_name);
        if let Some((was_running, was_quiet)) = prev.get(&a.id).copied() {
            if a.running && !was_running { out.push((a.id.clone(), name.clone(), "started".to_string())); }
            else if !a.running && was_running {
                let e = match a.exit_info.as_deref() { Some("exit 0") => "finished (exit 0)".to_string(), Some(i) => format!("stopped with {}", clean_name(i)), None => "stopped".to_string() };
                out.push((a.id.clone(), name.clone(), e));
            } else if quiet && !was_quiet { out.push((a.id.clone(), name.clone(), "quiet for 10 minutes".to_string())); }
            else if a.running && was_quiet && !quiet { out.push((a.id.clone(), name.clone(), "active again".to_string())); }
        }
        else if seeded && a.running { out.push((a.id.clone(), name.clone(), "started".to_string())); }
        next.insert(a.id.clone(), (a.running, quiet));
    }
    *prev = next;
    out
}
fn tl_push(evs: Vec<(String, String, String)>) {
    if evs.is_empty() { return; }
    if let Ok(mut tl) = TL.lock() {
        let t = now_s();
        for (id, name, event) in evs { tl.push(TlEvent { t, id, name, event }); }
        let n = tl.len(); if n > TL_KEEP { tl.drain(..n - TL_KEEP); }
    }
}
#[tauri::command]
pub fn timeline_list(window: tauri::WebviewWindow) -> Result<Vec<TlEvent>, String> {
    super::authorized(&window)?;
    let now = now_s();
    Ok(TL.lock().map(|t| t.iter().rev().filter(|x| now - x.t <= TL_AGE_S).cloned().collect()).unwrap_or_default())
}
#[tauri::command]
pub fn attention_list(window: tauri::WebviewWindow) -> Result<Vec<Attn>, String> {
    super::authorized(&window)?;
    let now = now_s();
    Ok(ATTN.lock().map(|a| a.iter().rev().filter(|x| now - x.t <= ATTN_AGE_S).cloned().collect()).unwrap_or_default())
}

// ---- Desktop notifications: one fixed `notify-send` call, no address, no secret. Off until switched on in Settings (a flag file in the app's data folder).
fn desktop_flag() -> Result<std::path::PathBuf, String> { Ok(super::app_data_dir()?.join("desktop-notify")) }
fn desktop_enabled() -> bool { desktop_flag().ok().and_then(|p| std::fs::symlink_metadata(p).ok()).is_some_and(|m| m.is_file()) }
fn desktop_program() -> std::ffi::OsString {
    #[cfg(feature = "workspace-e2e")]
    if let Some(p) = std::env::var_os("AGENT_APP_TEST_NOTIFYSEND") { return p; }
    "notify-send".into()
}
fn desktop_available() -> bool {
    let prog = desktop_program();
    let p = std::path::Path::new(&prog);
    if p.is_absolute() { return p.is_file(); }
    std::env::var_os("PATH").is_some_and(|v| std::env::split_paths(&v).any(|d| d.join(&prog).is_file()))
}
fn desktop_send(text: &str) -> Result<(), String> {
    {
        let mut s = DESK.lock().map_err(|_| "Desktop notification state unavailable")?;
        s.retain(|t| t.elapsed() < Duration::from_secs(3600));
        if s.len() >= DESK_PER_HOUR { return Err("Hourly limit reached (30 desktop notifications)".into()); }
        s.push(Instant::now());
    }
    let body: String = text.chars().filter(|c| !c.is_control()).map(|c| if c == '<' || c == '>' || c == '&' { ' ' } else { c }).take(300).collect();
    let mut c = Command::new(desktop_program());
    c.args(["--app-name=agentVIEW", "--urgency=normal", "--expire-time=15000", "--", "agentVIEW"]).arg(&body)
        .env_clear().env("PATH", std::env::var_os("PATH").unwrap_or_default()).env("LC_ALL", "C")
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    for k in ["DBUS_SESSION_BUS_ADDRESS", "XDG_RUNTIME_DIR", "DISPLAY", "WAYLAND_DISPLAY"] { if let Some(v) = std::env::var_os(k) { c.env(k, v); } }
    let mut child = c.spawn().map_err(|_| "notify-send is not installed")?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return if s.success() { Ok(()) } else { Err("The desktop did not accept the notification".into()) },
            Ok(None) if start.elapsed() < Duration::from_secs(5) => std::thread::sleep(Duration::from_millis(20)),
            _ => { let _ = child.kill(); let _ = child.wait(); return Err("The desktop notification timed out".into()); }
        }
    }
}
#[derive(Serialize)]
pub struct DesktopStatus { enabled: bool, available: bool, last: String }
static DESK_LAST: Mutex<String> = Mutex::new(String::new());
fn desktop_status() -> DesktopStatus { DesktopStatus { enabled: desktop_enabled(), available: desktop_available(), last: DESK_LAST.lock().map(|l| l.clone()).unwrap_or_default() } }
/// Read (both arguments empty), switch on or off (`enabled`), or send one fixed test message (`test`).
#[tauri::command]
pub fn desktop_notify(window: tauri::WebviewWindow, enabled: Option<bool>, test: Option<bool>) -> Result<DesktopStatus, String> {
    super::authorized(&window)?;
    if let Some(on) = enabled {
        let flag = desktop_flag()?;
        if on {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&flag).map_err(|_| "Could not save the setting")?;
        } else if std::fs::symlink_metadata(&flag).is_ok() { std::fs::remove_file(&flag).map_err(|_| "Could not save the setting")?; }
    }
    if test == Some(true) {
        let r = desktop_send("Test notification: desktop notifications are working.");
        if let Ok(mut l) = DESK_LAST.lock() { *l = match &r { Ok(()) => "Test notification sent".into(), Err(e) => format!("Test failed: {e}") }; }
        r?;
    }
    Ok(desktop_status())
}

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

/// New Claude Notification-hook lines become "waiting" messages. The agent is named after the registered session in that folder,
/// otherwise the folder name. The same session and kind is reported at most once a minute. Lines written before the app started are ignored.
fn hook_messages(path: Result<std::path::PathBuf, String>, label: &str, offset: &mut Option<u64>, seen: &mut HashMap<(String, String), Instant>, acts: &[Activity]) -> Vec<(&'static str, String)> {
    let Ok(path) = path else { return vec![] };
    let Some(off) = offset.as_mut() else { *offset = Some(super::hooksignal::file_len(&path)); return vec![] };
    seen.retain(|_, t| t.elapsed() < Duration::from_secs(60));
    let mut out = Vec::new();
    for s in super::hooksignal::read_new(&path, off) {
        let Some(what) = super::hooksignal::waiting_text(&s.kind) else { continue };
        if seen.insert((s.session.clone(), s.kind.clone()), Instant::now()).is_some() { continue; }
        let name = acts.iter().find(|a| !s.cwd.is_empty() && a.project == s.cwd).map(|a| clean_name(&a.display_name))
            .unwrap_or_else(|| clean_name(s.cwd.rsplit('/').find(|p| !p.is_empty()).unwrap_or("a session")));
        let note = if s.message.is_empty() { String::new() } else { format!(": \"{}\"", s.message.chars().take(140).collect::<String>()) };
        out.push(("waiting", format!("agentVIEW: {name} ({label}) {what}{note}")));
    }
    out
}

pub fn start_monitor(app: tauri::AppHandle) {
    use tauri::Manager;
    std::thread::spawn(move || {
        let mut seen: HashMap<String, Seen> = HashMap::new();
        let mut last = Instant::now();
        let mut sig_offset: Option<u64> = None;
        let mut codex_offset: Option<u64> = None;
        let mut mail_offset: Option<u64> = None;
        let mut board_known: Option<std::collections::HashSet<(String, String, i64)>> = None;
        let mut lim_sent: std::collections::HashSet<(&'static str, i64)> = std::collections::HashSet::new();
        let mut lim_started = false;
        let mut tl_prev: HashMap<String, (bool, bool)> = HashMap::new();
        let mut tl_seeded = false;
        let mut sig_seen: HashMap<(String, String), Instant> = HashMap::new();
        loop {
            std::thread::sleep(poll_interval());
            let dt = last.elapsed().min(Duration::from_secs(90)).as_millis() as i64;
            last = Instant::now();
            let live = app.state::<super::live::LiveState>();
            let acts = live.peek_activity();
            let mut msgs: Vec<(&'static str, String)> = Vec::new();
            // Plan-limit alerts go first so a burst of other messages cannot push them past the per-tick cap. A window already over the threshold when the app
            // starts is marked as known (not alerted again after every restart); the Overview still shows it.
            if let (Some(th), Some(l)) = (super::limits::threshold(), super::limits::read()) {
                let found = super::limits::crossings(&l, now_s(), th, &mut lim_sent);
                if lim_started { msgs.extend(found.into_iter().map(|m| ("limit", m))); }
            }
            lim_started = true;
            if let Some(acts) = &acts {
                tl_push(timeline_events(&mut tl_prev, acts, tl_seeded)); tl_seeded = true;
                let samples = super::timelog::sample(acts, dt);
                if !samples.is_empty() {
                    let state = app.state::<super::WorkspaceState>();
                    if let Ok(m) = state.0.as_ref() { if let Ok(mut s) = m.lock() { let _ = s.time_add(&samples); } }
                }
                msgs.extend(decide(&mut seen, acts));
            }
            msgs.extend(hook_messages(super::hooksignal::file(), "Claude Code", &mut sig_offset, &mut sig_seen, acts.as_deref().unwrap_or(&[])));
            msgs.extend(hook_messages(super::hooksignal::codex_file(), "Codex", &mut codex_offset, &mut sig_seen, acts.as_deref().unwrap_or(&[])));
            if let Ok(mp) = super::mailbox::file() {
                match mail_offset.as_mut() {
                    None => mail_offset = Some(super::mailbox::file_len(&mp)),
                    Some(off) => msgs.extend(super::mailbox::read_new(&mp, off).iter().take(5).map(|m| ("mailbox", super::mailbox::notice(m)))),
                }
            }
            if super::kanban::alert_enabled() { msgs.extend(super::kanban::new_blocked(&mut board_known, &super::kanban::all_boards()).into_iter().take(5).map(|m| ("board", m))); } else { board_known = None; }
            if msgs.is_empty() { continue; }
            for (kind, m) in &msgs { if *kind != "finished" { record(kind, m); } }
            if desktop_enabled() {
                for (kind, m) in msgs.iter().filter(|(k, _)| *k != "finished").take(5) {
                    let _ = kind;
                    let r = desktop_send(m);
                    if let Ok(mut l) = DESK_LAST.lock() { *l = match &r { Ok(()) => "Last desktop notification sent".into(), Err(e) => format!("Last desktop notification failed: {e}") }; }
                }
            }
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
    #[test] fn timeline_follows_state_changes_and_ignores_the_first_sight() {
        let mut prev = HashMap::new();
        assert!(timeline_events(&mut prev, &[act("a", true, Some(0), "running", None)], false).is_empty());
        assert!(timeline_events(&mut prev, &[act("a", true, Some(1000), "running", None)], true).is_empty());
        assert_eq!(timeline_events(&mut prev, &[act("a", true, Some(11 * 60000), "running", None)], true)[0].2, "quiet for 10 minutes");
        assert!(timeline_events(&mut prev, &[act("a", true, Some(12 * 60000), "running", None)], true).is_empty());
        assert_eq!(timeline_events(&mut prev, &[act("a", true, Some(100), "running", None)], true)[0].2, "active again");
        assert_eq!(timeline_events(&mut prev, &[act("a", false, None, "exited", Some("exit 1"))], true)[0].2, "stopped with exit 1");
        assert_eq!(timeline_events(&mut prev, &[act("a", true, Some(0), "running", None)], true)[0].2, "started");
        assert_eq!(timeline_events(&mut prev, &[act("a", false, None, "exited", Some("exit 0"))], true)[0].2, "finished (exit 0)");
    }
    #[test] fn names_are_cleaned() { assert_eq!(clean_name("a\nb\u{7}c"), "abc"); assert_eq!(clean_name(&"x".repeat(200)).len(), 80); }
}
