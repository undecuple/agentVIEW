//! Export and import bundle. Scope: preferences and the app-owned notes library, nothing else. Not included:
//! sessions, terminal content, conversations, usage records, manual-review labels, allow-lists, credentials or paths.
//! Bundles live only in the app's private exports folder; the UI never supplies a path, only a listed file name.
use agent_app_workspace::{notes::{Note, MAX_BODY}, settings::AppSettings};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::PathBuf;

const FORMAT: u32 = 1;
const MAX_BUNDLE: u64 = 24 * 1024 * 1024;
const MAX_PAGES: usize = 2000;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Page { shelf: String, title: String, body: String }
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload { settings: AppSettings, pages: Vec<Page> }
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle { format: u32, app: String, payload_sha256: String, payload: Payload }

#[derive(Serialize)]
pub struct BundleFile { name: String, size: u64 }
#[derive(Serialize)]
pub struct BundleSummary { name: String, app_version: String, pages: usize, shelves: usize, settings_included: bool, theme: String }
#[derive(Serialize)]
pub struct ImportResult { pages_added: usize, pages_renamed: usize, settings_applied: bool }

fn dir() -> Result<PathBuf, String> {
    let base = super::app_data_dir()?;
    if std::fs::canonicalize(&base).map_err(|_| "Data directory unavailable")? != base { return Err("Bundle storage cannot use symlinks".into()); }
    let d = base.join("exports");
    match std::fs::DirBuilder::new().mode(0o700).create(&d) { Ok(()) => (), Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (), Err(_) => return Err("Cannot create the exports folder".into()) }
    let m = std::fs::symlink_metadata(&d).map_err(|_| "Cannot inspect the exports folder")?;
    if !m.is_dir() || m.uid() != agent_app_core::sys::euid() || m.mode() & 0o077 != 0 { return Err("The exports folder must be private and owned by you".into()); }
    Ok(d)
}
fn valid_name(n: &str) -> bool {
    n.starts_with("agentview-bundle-") && n.ends_with(".json") && n.len() <= 80 && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_') && !n.contains("..")
}
fn digest(p: &Payload) -> Result<String, String> {
    let bytes = serde_json::to_vec(p).map_err(|_| "Cannot encode the bundle")?;
    Ok(Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}
fn read(name: &str) -> Result<Bundle, String> {
    if !valid_name(name) { return Err("Not a bundle file name".into()); }
    let path = dir()?.join(name);
    let m = std::fs::symlink_metadata(&path).map_err(|_| "That bundle is not in the exports folder")?;
    if !m.is_file() || m.nlink() != 1 || m.len() > MAX_BUNDLE { return Err("That bundle cannot be read (not a plain file, linked, or over 24 MB)".into()); }
    let mut text = String::new();
    std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW).open(&path).and_then(|f| f.take(MAX_BUNDLE).read_to_string(&mut text)).map_err(|_| "Cannot read that bundle")?;
    let b: Bundle = serde_json::from_str(&text).map_err(|_| "That file is not a valid agentVIEW bundle")?;
    if b.format != FORMAT { return Err("This bundle was made by a different format version".into()); }
    if digest(&b.payload)? != b.payload_sha256 { return Err("The bundle's integrity check failed; it was changed or damaged".into()); }
    if b.payload.pages.len() > MAX_PAGES || b.payload.pages.iter().any(|p| p.body.len() > MAX_BODY) { return Err("The bundle exceeds the notes limits".into()); }
    b.payload.settings.validate().map_err(|e| format!("The bundle's preferences are invalid: {e}"))?;
    Ok(b)
}

#[tauri::command]
pub fn bundle_export(window: tauri::WebviewWindow, state: tauri::State<super::WorkspaceState>) -> Result<BundleFile, String> {
    let (settings, notes) = super::with_store(&window, &state, |s| {
        let settings = s.settings_snapshot()?.settings;
        let mut pages = Vec::new();
        for m in s.notes_list("")? { let n: Note = s.note_get(m.id)?; pages.push(Page { shelf: n.shelf, title: n.title, body: n.body }); }
        Ok((settings, pages))
    })?;
    let payload = Payload { settings, pages: notes };
    let bundle = Bundle { format: FORMAT, app: env!("CARGO_PKG_VERSION").into(), payload_sha256: digest(&payload)?, payload };
    let bytes = serde_json::to_vec_pretty(&bundle).map_err(|_| "Cannot encode the bundle")?;
    if bytes.len() as u64 > MAX_BUNDLE { return Err("The bundle would exceed 24 MB".into()); }
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| "Clock unavailable")?.as_secs();
    let name = format!("agentview-bundle-{nonce}-{}.json", std::process::id());
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(dir()?.join(&name)).map_err(|_| "Cannot create the bundle")?;
    f.write_all(&bytes).and_then(|_| f.sync_all()).map_err(|_| "Cannot finish the bundle")?;
    Ok(BundleFile { name, size: bytes.len() as u64 })
}

#[tauri::command]
pub fn bundle_list(window: tauri::WebviewWindow) -> Result<Vec<BundleFile>, String> {
    super::authorized(&window)?;
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir()?).map_err(|_| "Cannot list the exports folder")?.flatten().take(500) {
        let name = e.file_name().to_string_lossy().into_owned();
        if !valid_name(&name) { continue; }
        if let Ok(m) = e.metadata() { if m.is_file() { out.push(BundleFile { name, size: m.len() }); } }
    }
    out.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(out)
}

#[tauri::command]
pub fn bundle_preview(window: tauri::WebviewWindow, name: String) -> Result<BundleSummary, String> {
    super::authorized(&window)?;
    let b = read(&name)?;
    let shelves = b.payload.pages.iter().map(|p| p.shelf.to_lowercase()).collect::<std::collections::BTreeSet<_>>().len();
    Ok(BundleSummary { name, app_version: b.app, pages: b.payload.pages.len(), shelves, settings_included: true, theme: b.payload.settings.theme })
}

/// Pages are always added as new pages (a title clash gets " (imported)"); nothing existing is overwritten or deleted.
/// Preferences replace the current ones only when asked, through the normal validated save.
#[tauri::command]
pub fn bundle_import(window: tauri::WebviewWindow, state: tauri::State<super::WorkspaceState>, name: String, include_settings: bool) -> Result<ImportResult, String> {
    super::authorized(&window)?;
    let b = read(&name)?;
    super::with_store(&window, &state, |s| {
        let existing = s.notes_list("")?;
        if existing.len() + b.payload.pages.len() > MAX_PAGES { return Err("Importing would exceed the 2000-page notes limit".into()); }
        let (mut added, mut renamed) = (0, 0);
        for p in &b.payload.pages {
            let clash = existing.iter().any(|m| m.shelf.eq_ignore_ascii_case(&p.shelf) && m.title.eq_ignore_ascii_case(&p.title));
            let title = if clash { renamed += 1; format!("{} (imported)", p.title.chars().take(100).collect::<String>()) } else { p.title.clone() };
            s.note_save(None, &p.shelf, &title, &p.body, None)?;
            added += 1;
        }
        let mut applied = false;
        if include_settings {
            let rev = s.settings_snapshot()?.revision;
            s.settings_save(rev, b.payload.settings.clone())?;
            applied = true;
        }
        Ok(ImportResult { pages_added: added, pages_renamed: renamed, settings_applied: applied })
    })
}
