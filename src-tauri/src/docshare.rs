//! Folders and files shared with the Documents page. The list lives in the app's own data folder (not in a hand-edited file) and is
//! only changed by a click in the main window. Reading is still read-only and goes through the same path policy as ever: the shared
//! entry is re-resolved and re-checked on every call, so a folder that was replaced or a symlink that was added later cannot widen access.
//! A list that cannot be read is treated as empty (fail closed); it never falls back to broader access.
use agent_app_core::{html::escape, images, markdown, policy::{self, Config}};
use serde::Serialize;
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const MAX_ENTRIES: usize = 100;
const STORE_CAP: u64 = 64 * 1024;
const LOG_CAP: u64 = 1024 * 1024;
const READ_CAP: usize = 256 * 1024;
const MAX_LISTED_SHOWN: usize = 500;
static WRITE: Mutex<()> = Mutex::new(());

/// The reason the startup config (allowlist.toml) did not load, if it did not. A missing file is normal and is not an error.
pub struct DocsNote(pub Arc<Option<String>>);

fn store_path(dir: &Path) -> PathBuf { dir.join("docs-shared.json") }

fn private_open(path: &Path, write: bool) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    if write { o.write(true).create(true).append(true); } else { o.read(true); }
    o.mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC).open(path)
}

fn clean(s: &str) -> String { s.chars().filter(|c| !c.is_control()).take(policy::MAX_SHARE_PATH).collect() }

/// The stored paths, or an error text when the file is not trustworthy. A missing file is an empty list.
pub fn read_store(dir: &Path) -> Result<Vec<String>, String> {
    let bad = || "The list of shared folders could not be read, so nothing is shared until it is fixed or removed".to_string();
    let f = match private_open(&store_path(dir), false) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(_) => return Err(bad()),
    };
    let m = f.metadata().map_err(|_| bad())?;
    if !m.is_file() || m.uid() != agent_app_core::sys::euid() || m.mode() & 0o077 != 0 || m.len() > STORE_CAP { return Err(bad()); }
    let mut data = Vec::new();
    f.take(STORE_CAP + 1).read_to_end(&mut data).map_err(|_| bad())?;
    let v: serde_json::Value = serde_json::from_slice(&data).map_err(|_| bad())?;
    if v.get("v").and_then(|x| x.as_i64()) != Some(1) { return Err(bad()); }
    let arr = v.get("paths").and_then(|p| p.as_array()).ok_or_else(bad)?;
    if arr.len() > MAX_ENTRIES { return Err(bad()); }
    arr.iter().map(|x| x.as_str().filter(|s| s.len() <= policy::MAX_SHARE_PATH && !s.chars().any(|c| c.is_control())).map(String::from).ok_or_else(bad)).collect()
}

/// Read-modify-write under a process lock and a file lock; the new list replaces the old one atomically (temp file, fsync, rename).
fn modify(dir: &Path, f: impl FnOnce(&mut Vec<String>) -> Result<(), String>) -> Result<(), String> {
    let _g = WRITE.lock().map_err(|_| "The shared list is busy")?;
    let lock = private_open(&dir.join("docs-shared.lock"), true).map_err(|_| "Cannot lock the shared list")?;
    use std::os::fd::AsRawFd;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 { return Err("Cannot lock the shared list".into()); }
    let mut list = read_store(dir)?;
    f(&mut list)?;
    let body = serde_json::to_vec_pretty(&serde_json::json!({"v": 1, "paths": list})).map_err(|_| "Cannot save the shared list")?;
    let tmp = dir.join("docs-shared.json.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut t = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&tmp).map_err(|_| "Cannot save the shared list")?;
    t.write_all(&body).and_then(|_| t.sync_all()).map_err(|_| "Cannot save the shared list")?;
    std::fs::rename(&tmp, store_path(dir)).map_err(|_| "Cannot save the shared list")?;
    if let Ok(d) = std::fs::File::open(dir) { let _ = d.sync_all(); }
    Ok(())
}

/// One line per change: time, action and path. No file content. Skipped silently once the log is large.
fn log_event(dir: &Path, action: &str, path: &str) {
    let p = dir.join("docs-share.log");
    if std::fs::symlink_metadata(&p).map(|m| m.len() > LOG_CAP).unwrap_or(false) { return; }
    if let Ok(mut f) = private_open(&p, true) {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = writeln!(f, "{t} {action} {}", clean(path));
    }
}

#[derive(Serialize, Clone)]
pub struct Entry { pub path: String, pub short: String, pub source: &'static str, pub ok: bool, pub reason: Option<String>, pub kind: Option<&'static str> }

pub struct Effective { pub cfg: Config, pub shared: Vec<Entry>, pub store_note: Option<String> }

fn kind_of(p: &Path) -> &'static str { if p.is_dir() { "folder" } else { "file" } }

/// The allow list in force right now: the entries of allowlist.toml plus the app's own list, each checked again against the rules and
/// against where it points today. An entry that no longer resolves to the very path that was approved is refused, never followed.
pub fn effective(base: &Config, dir: &Path) -> Effective {
    let mut cfg = base.clone();
    if cfg.deny.is_empty() { cfg.deny = policy::builtin().deny; }
    let from_file: Vec<PathBuf> = std::mem::take(&mut cfg.allow);
    let mut shared = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut consider = |text: String, source: &'static str, cfg: &mut Config, shared: &mut Vec<Entry>| {
        let short = policy::short(&text);
        match policy::validate_allow(&text, true) {
            Ok(real) if real == Path::new(&text) => {
                if !seen.contains(&real) { seen.push(real.clone()); cfg.allow.push(real.clone()); }
                shared.push(Entry { path: text, short, source, ok: true, reason: None, kind: Some(kind_of(&real)) });
            }
            Ok(_) => shared.push(Entry { path: text, short, source, ok: false, reason: Some("This path now points somewhere else than when it was shared. Remove it and share it again if you still want it.".into()), kind: None }),
            Err(e) => shared.push(Entry { path: text, short, source, ok: false, reason: Some(e), kind: None }),
        }
    };
    for p in from_file { consider(p.to_string_lossy().into_owned(), "config file", &mut cfg, &mut shared); }
    let mut store_note = None;
    match read_store(dir) {
        Ok(list) => for p in list { consider(p, "app", &mut cfg, &mut shared); },
        Err(e) => store_note = Some(e),
    }
    Effective { cfg, shared, store_note }
}

fn data_dir() -> Result<PathBuf, String> { super::app_data_dir() }

/// What the viewer window (the classic Documents window) uses for each request.
pub fn viewer_config(base: &Config) -> Config {
    match data_dir() { Ok(d) => effective(base, &d).cfg, Err(_) => { let mut c = base.clone(); c.allow.clear(); c } }
}

#[derive(Serialize)]
pub struct FileItem { pub path: String, pub short: String, pub image: bool }
#[derive(Serialize)]
pub struct DocsView { pub shared: Vec<Entry>, pub files: Vec<FileItem>, pub more: bool, pub deny: Vec<String>, pub config_note: Option<String>, pub store_note: Option<String> }

#[tauri::command]
pub fn docs_list(window: tauri::WebviewWindow, base: tauri::State<Arc<Config>>, note: tauri::State<DocsNote>) -> Result<DocsView, String> {
    super::authorized(&window)?;
    let eff = effective(&base, &data_dir()?);
    let (all, cut) = policy::listing_capped(&eff.cfg);
    let more = cut || all.len() > MAX_LISTED_SHOWN;
    let files = all.iter().take(MAX_LISTED_SHOWN).map(|p| { let s = p.to_string_lossy().into_owned(); FileItem { short: policy::short(&s), image: images::is_image_ext(&s), path: s } }).collect();
    let mut deny = eff.cfg.deny.clone(); deny.sort();
    Ok(DocsView { shared: eff.shared, files, more, deny, config_note: (*note.0).clone(), store_note: eff.store_note })
}

#[derive(Serialize)]
pub struct Check { pub real: String, pub short: String, pub kind: &'static str, pub already: bool }

/// Step one of sharing: resolve the typed path and say exactly what would be shared. Writes nothing.
#[tauri::command]
pub fn docs_check(window: tauri::WebviewWindow, base: tauri::State<Arc<Config>>, path: String) -> Result<Check, String> {
    super::authorized(&window)?;
    let real = policy::validate_allow(path.trim(), true)?;
    let eff = effective(&base, &data_dir()?);
    let real_s = real.to_string_lossy().into_owned();
    Ok(Check { short: policy::short(&real_s), kind: kind_of(&real), already: eff.cfg.allow.contains(&real), real: real_s })
}

/// Step two: `expect` is the resolved path the person was shown. If the typed path now resolves anywhere else, nothing is saved.
#[tauri::command]
pub fn docs_add(window: tauri::WebviewWindow, path: String, expect: String) -> Result<(), String> {
    super::authorized(&window)?;
    let dir = data_dir()?;
    let real = policy::validate_allow(path.trim(), true)?;
    let real_s = real.to_string_lossy().into_owned();
    if real_s != expect { return Err("The path changed since you checked it. Check it again.".into()); }
    modify(&dir, |list| {
        if list.iter().any(|x| *x == real_s) { return Ok(()); }
        if list.len() >= MAX_ENTRIES { return Err(format!("At most {MAX_ENTRIES} folders or files can be shared")); }
        list.push(real_s.clone());
        Ok(())
    })?;
    log_event(&dir, "add", &real_s);
    Ok(())
}

#[tauri::command]
pub fn docs_remove(window: tauri::WebviewWindow, path: String) -> Result<(), String> {
    super::authorized(&window)?;
    let dir = data_dir()?;
    modify(&dir, |list| {
        let before = list.len();
        list.retain(|x| *x != path);
        if list.len() == before { Err("That entry is not in the app's list. Entries from allowlist.toml are removed by editing that file.".into()) } else { Ok(()) }
    })?;
    log_event(&dir, "remove", &path);
    Ok(())
}

#[derive(Serialize)]
pub struct DocText { pub name: String, pub html: String, pub truncated: bool }

#[tauri::command]
pub fn docs_read(window: tauri::WebviewWindow, base: tauri::State<Arc<Config>>, path: String) -> Result<DocText, String> {
    super::authorized(&window)?;
    let eff = effective(&base, &data_dir()?);
    if images::is_image_ext(&path) { return Err("Images are not shown on this page".into()); }
    let (real, data, more) = policy::read_bytes(&path, &eff.cfg, READ_CAP).map_err(|_| "That file is not available")?;
    if data.contains(&0) { return Err("Binary files are not shown".into()); }
    let text = policy::redact(&String::from_utf8_lossy(&data));
    let rs = real.to_string_lossy().into_owned();
    let md = matches!(real.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(), Some("md") | Some("markdown"));
    let html = if md { markdown::render(&text) } else { format!("<pre>{}</pre>", escape(&text)) };
    Ok(DocText { name: policy::short(&rs), html, truncated: more })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, DirBuilderExt};
    fn private_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("agentview-docshare-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&d).unwrap();
        std::fs::canonicalize(&d).unwrap()
    }
    fn write_raw(dir: &Path, body: &str, mode: u32) {
        let p = store_path(dir);
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(mode)).unwrap();
    }
    #[test]
    fn corrupt_unsafe_or_symlinked_store_fails_closed() {
        let d = private_dir("store");
        assert_eq!(read_store(&d).unwrap(), Vec::<String>::new(), "no file is an empty list");
        write_raw(&d, "{ not json", 0o600); assert!(read_store(&d).is_err(), "corrupt");
        write_raw(&d, r#"{"v":2,"paths":[]}"#, 0o600); assert!(read_store(&d).is_err(), "unknown version");
        write_raw(&d, r#"{"v":1,"paths":[1]}"#, 0o600); assert!(read_store(&d).is_err(), "not strings");
        write_raw(&d, r#"{"v":1,"paths":["/tmp/x"]}"#, 0o644); assert!(read_store(&d).is_err(), "group/other readable");
        write_raw(&d, r#"{"v":1,"paths":["/tmp/x"]}"#, 0o600); assert_eq!(read_store(&d).unwrap().len(), 1);
        let real = d.join("real.json"); std::fs::rename(store_path(&d), &real).unwrap();
        symlink(&real, store_path(&d)).unwrap(); assert!(read_store(&d).is_err(), "symlinked store refused");
        // A broken store gives no access at all, whatever the base config says.
        let share = d.join("shared"); std::fs::create_dir(&share).unwrap(); std::fs::write(share.join("a.md"), "x").unwrap();
        let e = effective(&Config { deny: vec![], ..Default::default() }, &d);
        assert!(e.cfg.allow.is_empty() && e.store_note.is_some(), "fails closed");
        assert!(!e.cfg.deny.is_empty(), "the built-in deny list is applied when the config has none");
        let _ = std::fs::remove_dir_all(&d);
    }
    #[test]
    fn writes_are_private_atomic_and_logged_and_the_swapped_folder_is_refused() {
        let d = private_dir("write");
        let share = d.join("shared"); std::fs::create_dir(&share).unwrap(); std::fs::write(share.join("a.md"), "x").unwrap();
        let other = d.join("other"); std::fs::create_dir(&other).unwrap(); std::fs::write(other.join("b.md"), "y").unwrap();
        let s = share.to_string_lossy().into_owned();
        modify(&d, |l| { l.push(s.clone()); Ok(()) }).unwrap();
        let m = std::fs::metadata(store_path(&d)).unwrap();
        assert_eq!(m.mode() & 0o777, 0o600, "store is 0600");
        assert!(!d.join("docs-shared.json.tmp").exists(), "no temp file is left behind");
        log_event(&d, "add", &s); assert!(std::fs::read_to_string(d.join("docs-share.log")).unwrap().contains(&format!("add {s}")));
        let e = effective(&Config::default(), &d);
        assert!(e.shared[0].ok && e.cfg.allow == vec![share.clone()]);
        assert_eq!(policy::listing(&e.cfg), vec![share.join("a.md")]);
        // The shared folder is replaced by a symlink to another folder: the entry is refused and nothing from either folder is readable.
        std::fs::remove_dir_all(&share).unwrap(); symlink(&other, &share).unwrap();
        let e = effective(&Config::default(), &d);
        assert!(!e.shared[0].ok && e.cfg.allow.is_empty(), "a swapped folder is refused: {:?}", e.shared[0].reason);
        assert!(policy::listing(&e.cfg).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
    #[test]
    fn a_symlink_inside_a_shared_folder_cannot_leave_it() {
        let d = private_dir("escape");
        let share = d.join("shared"); std::fs::create_dir(&share).unwrap();
        let outside = d.join("outside.md"); std::fs::write(&outside, "secret-ish").unwrap();
        symlink(&outside, share.join("link.md")).unwrap();
        std::fs::write(share.join("ok.md"), "fine").unwrap();
        modify(&d, |l| { l.push(share.to_string_lossy().into_owned()); Ok(()) }).unwrap();
        let e = effective(&Config::default(), &d);
        assert_eq!(policy::listing(&e.cfg), vec![share.join("ok.md")], "the link is not listed");
        assert!(policy::read_bytes(&share.join("link.md").to_string_lossy(), &e.cfg, 100).is_err(), "and not readable");
        let _ = std::fs::remove_dir_all(&d);
    }
}
