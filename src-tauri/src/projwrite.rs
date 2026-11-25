//! Consent-gated writes into a registered session's project folder. This is the only place the app writes outside its own data folder.
//! Narrow on purpose: only a fixed set of file names (the app's own onboarding and handoff documents), only directly inside the folder of a session
//! the person registered, never through a symlink or hard link, only after a preview whose hash must still match, and only with an explicit confirm.
//! The existing file can be backed up first, into the app's own `backups/` folder (mode 0700). The replace is atomic (temporary file, then rename).
use agent_app_core::{policy, Config};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{atomic::{AtomicU64, Ordering}, Arc, Mutex};
use std::time::{Duration, Instant};

static WRITE_LOCK: Mutex<()> = Mutex::new(());
static TMP_N: AtomicU64 = AtomicU64::new(0);
/// The last preview the page asked for (session, name, hash of the new text, hash of the file it saw). A write must match it and arrive within ten minutes.
struct Seen { id: String, name: String, content_hash: String, current_hash: String, at: Instant }
static PREVIEWED: Mutex<Option<Seen>> = Mutex::new(None);

pub const NAMES: [&str; 4] = ["CLAUDE.md", "AGENTS.md", "HANDOFF.md", "BOARD.md"];
const MAX: usize = 256 * 1024;
const SHOW: usize = 64 * 1024;

fn sha(b: &[u8]) -> String { Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect() }

#[derive(Serialize)]
pub struct Preview { target: String, name: String, exists: bool, current_hash: String, current_size: u64, current_text: String, new_hash: String, same: bool, note: &'static str }
#[derive(Serialize)]
pub struct Written { target: String, backup: Option<String>, new_hash: String }

/// The folder must be a real directory owned by you, not the filesystem root, and no path component may match a deny name.
fn check_dir(dir: &Path, cfg: &Config) -> Result<PathBuf, String> {
    let d = std::fs::canonicalize(dir).map_err(|_| "The session's folder is unavailable")?;
    let m = std::fs::metadata(&d).map_err(|_| "The session's folder is unavailable")?;
    if !m.is_dir() || d == Path::new("/") { return Err("This folder cannot be written to".into()); }
    if m.uid() != agent_app_core::sys::euid() { return Err("The folder must be owned by you".into()); }
    if d.to_str().is_none_or(|s| super::gitview::denied_name(s, cfg)) { return Err("This folder matches a deny rule".into()); }
    Ok(d)
}
fn check_name(name: &str) -> Result<(), String> { if NAMES.contains(&name) { Ok(()) } else { Err("That file name is not allowed".into()) } }
fn check_content(c: &str) -> Result<(), String> {
    if c.is_empty() || c.len() > MAX { return Err("The document is empty or larger than 256 KB".into()); }
    if c.contains('\0') { return Err("The document contains a NUL byte".into()); }
    Ok(())
}
/// The current file: None if absent. A symlink, a non-regular file, a hard-linked file or one over the size cap is refused.
fn current(dir: &Path, name: &str) -> Result<Option<(Vec<u8>, std::fs::Metadata)>, String> {
    let p = dir.join(name);
    let l = match std::fs::symlink_metadata(&p) { Ok(m) => m, Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None), Err(_) => return Err("Cannot inspect the file".into()) };
    if l.file_type().is_symlink() { return Err("The file is a symbolic link; the app does not write through links".into()); }
    if !l.is_file() { return Err("The path exists but is not a regular file".into()); }
    if l.nlink() > 1 { return Err("The file has several hard links; the app does not replace it".into()); }
    if l.uid() != agent_app_core::sys::euid() { return Err("The existing file is owned by someone else; the app does not replace it".into()); }
    if l.len() as usize > MAX { return Err("The existing file is larger than 256 KB".into()); }
    let mut f = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(&p).map_err(|_| "Cannot open the file")?;
    let m = f.metadata().map_err(|_| "Cannot inspect the file")?;
    if !m.is_file() { return Err("The path exists but is not a regular file".into()); }
    if m.ino() != l.ino() || m.dev() != l.dev() { return Err("The file changed while it was being read".into()); }
    let mut b = Vec::new();
    (&mut f).take(MAX as u64 + 1).read_to_end(&mut b).map_err(|_| "Cannot read the file")?;
    if b.len() > MAX { return Err("The existing file is larger than 256 KB".into()); }
    Ok(Some((b, m)))
}

pub fn preview(dir: &Path, cfg: &Config, name: &str, content: &str) -> Result<Preview, String> {
    check_name(name)?; check_content(content)?;
    let d = check_dir(dir, cfg)?;
    let cur = current(&d, name)?;
    let new_hash = sha(content.as_bytes());
    let (exists, current_hash, current_size, current_text) = match &cur {
        Some((b, _)) => (true, sha(b), b.len() as u64, policy::redact(&String::from_utf8_lossy(&b[..b.len().min(SHOW)]))),
        None => (false, String::new(), 0, String::new()),
    };
    Ok(Preview { target: policy::short(&d.join(name).to_string_lossy()), name: name.into(), exists, same: current_hash == new_hash, current_hash, current_size, current_text, new_hash,
        note: "Nothing has been written. Writing needs your confirmation." })
}

/// Writes after the checks. `expected_hash` is the hash the preview saw ("" for a new file); a changed file is refused. `backup_root` None means no backup.
pub fn write(dir: &Path, cfg: &Config, name: &str, content: &str, expected_hash: &str, backup_root: Option<&Path>) -> Result<Written, String> {
    check_name(name)?; check_content(content)?;
    let _guard = WRITE_LOCK.lock().map_err(|_| "Another write is in progress")?;
    let d = check_dir(dir, cfg)?;
    let cur = current(&d, name)?;
    match (&cur, expected_hash) {
        (Some((b, _)), h) if sha(b) != h => return Err("The file changed since the preview (or was created meanwhile). Preview it again.".into()),
        (None, h) if !h.is_empty() => return Err("The file disappeared since the preview. Preview it again.".into()),
        _ => {}
    }
    let target = d.join(name);
    let mut backup_name = None;
    if let (Some((old, _)), Some(root)) = (&cur, backup_root) {
        use std::os::unix::fs::DirBuilderExt;
        match std::fs::DirBuilder::new().mode(0o700).create(root) { Ok(()) => {}, Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}, Err(_) => return Err("Cannot create the backups folder; nothing was written".into()) }
        let rm = std::fs::symlink_metadata(root).map_err(|_| "Cannot inspect the backups folder")?;
        if !rm.is_dir() || rm.uid() != agent_app_core::sys::euid() || rm.mode() & 0o077 != 0 { return Err("The backups folder must be private and owned by you; nothing was written".into()); }
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|t| t.as_secs()).unwrap_or(0);
        let slug: String = d.to_string_lossy().chars().rev().take(40).collect::<Vec<_>>().into_iter().rev().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        let bn = format!("{secs}_{}_{slug}_{name}", &sha(old)[..8]);
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(root.join(&bn)).map_err(|_| "Cannot create the backup; nothing was written")?;
        f.write_all(old).and_then(|_| f.sync_all()).map_err(|_| "Cannot write the backup; nothing was written")?;
        let line = format!("{}\n", serde_json::json!({"t": secs, "file": target.to_string_lossy(), "backup": bn, "sha256": sha(old)}));
        let rec = std::fs::OpenOptions::new().append(true).create(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(root.join("manifest.jsonl")).and_then(|mut mf| mf.write_all(line.as_bytes()));
        if rec.is_err() { let _ = std::fs::remove_file(root.join(&bn)); return Err("Cannot record the backup in the manifest; nothing was written".into()); }
        backup_name = Some(bn);
    }
    let new_hash = sha(content.as_bytes());
    match &cur {
        None => {
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o644).custom_flags(libc::O_NOFOLLOW).open(&target).map_err(|_| "Cannot create the file (it may have appeared meanwhile)")?;
            if f.write_all(content.as_bytes()).and_then(|_| f.sync_all()).is_err() { drop(f); let _ = std::fs::remove_file(&target); return Err("Cannot write the file; the incomplete file was removed".into()); }
        }
        Some((_, m)) => {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|t| t.subsec_nanos()).unwrap_or(0);
            let tmp = d.join(format!(".{name}.agentview-{}-{}-{nanos}", std::process::id(), TMP_N.fetch_add(1, Ordering::Relaxed)));
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&tmp).map_err(|_| "Cannot create a temporary file in the folder; nothing was changed")?;
            let r = (|| -> Result<(), &'static str> {
                f.write_all(content.as_bytes()).map_err(|_| "write")?;
                f.set_permissions(std::fs::Permissions::from_mode(m.mode() & 0o777)).map_err(|_| "mode")?;
                let _ = std::os::unix::fs::fchown(&f, None, Some(m.gid())); // keep the group when we may; ACLs and extended attributes are not carried over
                f.sync_all().map_err(|_| "sync")?;
                // Last check before the swap: the file must still be the one that was previewed (and backed up).
                match current(&d, name) { Ok(Some((b, _))) if sha(&b) == expected_hash => {}, _ => return Err("changed"), }
                std::fs::rename(&tmp, &target).map_err(|_| "rename")
            })();
            if let Err(why) = r {
                let _ = std::fs::remove_file(&tmp);
                return Err(if why == "changed" { "The file changed while it was being written; the old file was left as it was. Preview it again.".into() } else { "Cannot replace the file; the old file was left as it was".to_string() });
            }
            if let Ok(dh) = std::fs::File::open(&d) { let _ = dh.sync_all(); }
        }
    }
    let back = std::fs::read(&target).map_err(|_| "The file was written but could not be read back")?;
    if sha(&back) != new_hash { return Err("The file on disk does not match what was written. Check it.".into()); }
    Ok(Written { target: policy::short(&target.to_string_lossy()), backup: backup_name, new_hash })
}

fn backups_root() -> Result<PathBuf, String> { Ok(super::app_data_dir()?.join("backups")) }

#[tauri::command]
pub async fn project_file_preview(window: tauri::WebviewWindow, state: tauri::State<'_, super::live::LiveState>, cfg: tauri::State<'_, Arc<Config>>, id: String, name: String, content: String) -> Result<Preview, String> {
    let dir = super::live::session_dir(window, &state, &id).await?;
    let cfg = Arc::clone(&cfg);
    let (sid, n, c) = (id.clone(), name.clone(), content.clone());
    let p = tauri::async_runtime::spawn_blocking(move || preview(Path::new(&dir), &cfg, &name, &content)).await.map_err(|_| "Preview failed".to_string())??;
    if let Ok(mut s) = PREVIEWED.lock() { *s = Some(Seen { id: sid, name: n, content_hash: sha(c.as_bytes()), current_hash: p.current_hash.clone(), at: Instant::now() }); }
    Ok(p)
}
#[tauri::command]
pub async fn project_file_write(window: tauri::WebviewWindow, state: tauri::State<'_, super::live::LiveState>, cfg: tauri::State<'_, Arc<Config>>, id: String, name: String, content: String, expected_hash: String, backup: bool, confirm: bool) -> Result<Written, String> {
    if !confirm { return Err("Writing needs your explicit confirmation".into()); }
    // The write must be for the exact target, text and file state of the last preview (taken once, valid for ten minutes).
    {
        let seen = PREVIEWED.lock().map_err(|_| "Preview state unavailable")?.take();
        let ok = seen.is_some_and(|s| s.id == id && s.name == name && s.content_hash == sha(content.as_bytes()) && s.current_hash == expected_hash && s.at.elapsed() < Duration::from_secs(600));
        if !ok { return Err("Preview this exact text and target first (the preview is valid for ten minutes and for one write)".into()); }
    }
    let dir = super::live::session_dir(window, &state, &id).await?;
    let cfg = Arc::clone(&cfg);
    let root = if backup { Some(backups_root()?) } else { None };
    tauri::async_runtime::spawn_blocking(move || write(Path::new(&dir), &cfg, &name, &content, &expected_hash, root.as_deref())).await.map_err(|_| "Write failed".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cfg() -> Config { Config { deny: vec![".env".into()], ..Default::default() } }
    #[test] fn creates_replaces_backs_up_and_refuses_stale_links_and_names() {
        let d = tempfile::tempdir().unwrap(); let b = tempfile::tempdir().unwrap(); let root = b.path().join("backups");
        let dir = d.path();
        assert!(write(dir, &cfg(), "evil.sh", "x", "", None).is_err(), "names outside the fixed list are refused");
        assert!(write(dir, &cfg(), "CLAUDE.md", "", "", None).is_err(), "empty content refused");
        let p = preview(dir, &cfg(), "CLAUDE.md", "# one\n").unwrap(); assert!(!p.exists && p.current_hash.is_empty());
        assert!(write(dir, &cfg(), "CLAUDE.md", "# one\n", "deadbeef", None).is_err(), "a hash for a file that does not exist is refused");
        let w = write(dir, &cfg(), "CLAUDE.md", "# one\n", "", Some(&root)).unwrap(); assert!(w.backup.is_none(), "no backup when nothing existed");
        assert_eq!(std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap(), "# one\n");
        std::fs::set_permissions(dir.join("CLAUDE.md"), std::fs::Permissions::from_mode(0o640)).unwrap();
        let p2 = preview(dir, &cfg(), "CLAUDE.md", "# two\n").unwrap(); assert!(p2.exists && !p2.same && p2.current_text == "# one\n");
        assert!(write(dir, &cfg(), "CLAUDE.md", "# two\n", "", None).is_err(), "an existing file needs the preview hash");
        assert!(write(dir, &cfg(), "CLAUDE.md", "# two\n", &sha(b"other"), None).is_err(), "a stale hash is refused");
        let w2 = write(dir, &cfg(), "CLAUDE.md", "# two\n", &p2.current_hash, Some(&root)).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("CLAUDE.md")).unwrap(), "# two\n");
        assert_eq!(std::fs::metadata(dir.join("CLAUDE.md")).unwrap().mode() & 0o777, 0o640, "mode is kept");
        let bn = w2.backup.unwrap(); assert_eq!(std::fs::read_to_string(root.join(&bn)).unwrap(), "# one\n");
        assert_eq!(std::fs::metadata(root.join(&bn)).unwrap().mode() & 0o777, 0o600);
        assert!(std::fs::read_to_string(root.join("manifest.jsonl")).unwrap().contains(&bn));
        assert!(std::fs::read_dir(dir).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().contains("agentview-")), "no temporary file is left");
    }
    #[test] fn recheck_before_swap_and_unique_temp_names() {
        let d = tempfile::tempdir().unwrap(); let dir = d.path();
        std::fs::write(dir.join("AGENTS.md"), "old").unwrap();
        let h = sha(b"old");
        let a = write(dir, &cfg(), "AGENTS.md", "new a", &h, None).unwrap();
        assert!(a.target.ends_with("AGENTS.md"));
        // the same preview hash is now stale, so a second writer is refused instead of overwriting
        assert!(write(dir, &cfg(), "AGENTS.md", "new b", &h, None).is_err());
        assert_eq!(std::fs::read_to_string(dir.join("AGENTS.md")).unwrap(), "new a");
        assert!(std::fs::read_dir(dir).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().contains("agentview-")));
    }
    #[test] fn refuses_symlinks_hard_links_and_denied_folders() {
        let d = tempfile::tempdir().unwrap(); let dir = d.path();
        std::fs::write(dir.join("real.txt"), "secret target").unwrap();
        std::os::unix::fs::symlink(dir.join("real.txt"), dir.join("AGENTS.md")).unwrap();
        assert!(preview(dir, &cfg(), "AGENTS.md", "x").is_err() && write(dir, &cfg(), "AGENTS.md", "x", "", None).is_err());
        assert_eq!(std::fs::read_to_string(dir.join("real.txt")).unwrap(), "secret target", "the link target was not touched");
        std::fs::write(dir.join("HANDOFF.md"), "a").unwrap(); std::fs::hard_link(dir.join("HANDOFF.md"), dir.join("other")).unwrap();
        assert!(preview(dir, &cfg(), "HANDOFF.md", "x").is_err());
        let denied = d.path().join(".env"); std::fs::create_dir(&denied).unwrap();
        assert!(preview(&denied, &cfg(), "CLAUDE.md", "x").is_err(), "a folder matching a deny name is refused");
        assert!(preview(Path::new("/"), &cfg(), "CLAUDE.md", "x").is_err());
    }
}
