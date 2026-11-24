//! Backup of the app's own exports folder with rclone. rclone is a fixed program with fixed argument lists:
//!   rclone listremotes
//!   rclone copy <exports folder> <remote>:agentview-backup/exports --immutable --max-depth 1 [--dry-run]
//! `copy` never deletes or overwrites at the destination. The remote must be one that `listremotes` printed; names
//! only are shown, never rclone's configuration or its raw output. A real copy is accepted only after a dry run
//! for the same remote in this app session (valid for ten minutes), and the page asks for a second confirming click.
use serde::Serialize;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const CAP: u64 = 256 * 1024;
static PREVIEWED: Mutex<Option<(String, Instant)>> = Mutex::new(None);

#[derive(Serialize)]
pub struct Remotes { installed: bool, remotes: Vec<String> }
#[derive(Serialize)]
pub struct Backup { dry_run: bool, files: Vec<String>, count: usize, ok: bool, message: String }

fn program() -> std::ffi::OsString {
    #[cfg(feature = "workspace-e2e")]
    if let Some(p) = std::env::var_os("AGENT_APP_TEST_RCLONE") { return p; }
    "rclone".into()
}
fn valid_remote(n: &str) -> bool { !n.is_empty() && n.len() <= 40 && n.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-')) && !n.starts_with('-') }
fn safe_file(n: &str) -> bool { !n.is_empty() && n.len() <= 80 && n.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-')) }

/// Returns (stderr text, stdout text, exit code), or None when rclone is missing or runs over the limit.
fn run(args: &[&std::ffi::OsStr], wait: Duration) -> Option<(String, String, i32)> {
    let mut c = Command::new(program());
    c.args(args).env_clear().env("PATH", std::env::var_os("PATH").unwrap_or_default()).env("LC_ALL", "C")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(h) = std::env::var_os("HOME") { c.env("HOME", h); }
    if let Some(x) = std::env::var_os("XDG_CONFIG_HOME") { c.env("XDG_CONFIG_HOME", x); }
    let mut child = c.spawn().ok()?;
    let (mut o, mut e) = (child.stdout.take()?, child.stderr.take()?);
    let ro = std::thread::spawn(move || { let mut s = String::new(); let _ = (&mut o).take(CAP).read_to_string(&mut s); s });
    let re = std::thread::spawn(move || { let mut s = String::new(); let _ = (&mut e).take(CAP).read_to_string(&mut s); s });
    let start = Instant::now();
    let st = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() < wait => std::thread::sleep(Duration::from_millis(20)),
            _ => { let _ = child.kill(); let _ = child.wait(); return None; }
        }
    };
    Some((re.join().ok()?, ro.join().ok()?, st.code().unwrap_or(-1)))
}

fn names() -> Option<Vec<String>> {
    let (_, out, code) = run(&["listremotes".as_ref()], Duration::from_secs(10))?;
    if code != 0 { return None; }
    Some(out.lines().filter_map(|l| l.strip_suffix(':')).filter(|n| valid_remote(n)).take(100).map(String::from).collect())
}

#[tauri::command]
pub fn rclone_remotes(window: tauri::WebviewWindow) -> Result<Remotes, String> {
    super::authorized(&window)?;
    Ok(match names() { Some(remotes) => Remotes { installed: true, remotes }, None => Remotes { installed: false, remotes: vec![] } })
}

/// Names of files rclone reports as copied or skipped, from lines like `... NOTICE: name.json: Skipped copy as --dry-run`.
fn files_from(stderr: &str) -> Vec<String> {
    let mut v: Vec<String> = stderr.lines().filter(|l| l.contains("Skipped copy as --dry-run") || l.contains("Copied (new)"))
        .filter_map(|l| { let after = l.split_once(" : ").or_else(|| l.split_once(": "))?.1; let name = after.split(": ").next()?; Some(name.trim().to_string()) })
        .filter(|n| safe_file(n)).take(500).collect();
    v.sort(); v.dedup(); v
}

#[tauri::command]
pub fn rclone_backup(window: tauri::WebviewWindow, remote: String, apply: bool) -> Result<Backup, String> {
    super::authorized(&window)?;
    if !valid_remote(&remote) || !names().ok_or("rclone is not available")?.contains(&remote) { return Err("That is not one of your rclone remotes".into()); }
    if apply {
        let mut g = PREVIEWED.lock().map_err(|_| "Backup state unavailable")?;
        let ok = g.as_ref().is_some_and(|(r, t)| *r == remote && t.elapsed() < Duration::from_secs(600));
        if !ok { return Err("Run the dry run for this remote first".into()); }
        *g = None; // one real copy per dry run
    }
    let src = super::bundle::dir()?;
    let dst = format!("{remote}:agentview-backup/exports");
    let mut args: Vec<&std::ffi::OsStr> = vec!["copy".as_ref(), src.as_os_str(), dst.as_ref(), "--immutable".as_ref(), "--max-depth".as_ref(), "1".as_ref(), "--log-level".as_ref(), "INFO".as_ref(), "--stats".as_ref(), "0".as_ref()];
    if !apply { args.push("--dry-run".as_ref()); }
    let Some((err, _, code)) = run(&args, Duration::from_secs(if apply { 300 } else { 60 })) else { return Err("rclone did not finish in time or could not start".into()) };
    let files = files_from(&err);
    let count = files.len();
    if code != 0 { return Ok(Backup { dry_run: !apply, files, count, ok: false, message: format!("rclone stopped with exit code {code}. Its output is not shown; run `rclone copy` yourself to see details.") }); }
    if !apply { *PREVIEWED.lock().map_err(|_| "Backup state unavailable")? = Some((remote, Instant::now())); }
    Ok(Backup { dry_run: !apply, files, count, ok: true, message: if apply { format!("Copied {count} new file(s). Nothing was deleted or overwritten.") } else { format!("Dry run: {count} file(s) would be copied. Nothing was changed.") } })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn parses_dry_run_lines() {
        let s = "2026/11/23 10:00:00 NOTICE: agentview-bundle-1.json: Skipped copy as --dry-run\n2026/11/23 10:00:00 NOTICE: ../x/..y: Skipped copy as --dry-run\nother line\n";
        assert_eq!(files_from(s), vec!["agentview-bundle-1.json".to_string()]);
        assert_eq!(files_from("2026/01/01 10:00:00 INFO  : a.json: Copied (new)"), vec!["a.json".to_string()]);
    }
    #[test] fn remote_names() { assert!(valid_remote("my-drive_1") && !valid_remote("-x") && !valid_remote("a:b") && !valid_remote("a b") && !valid_remote("")); }
}
