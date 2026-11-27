//! Remembered CLI inspection. When the person presses "Inspect" the app records what it reviewed (provider, canonical
//! executable path, version string, file identity, a SHA-256 of the file and the two capability flags) in the sessions
//! store. After a restart the record is restored ONLY if the same file is still there unchanged: same canonical path,
//! a regular file that is not a symlink, executable, not writable by group or others, same identity
//! (device, inode, size, modification time) and the same SHA-256. Anything else falls back to asking the person to
//! inspect again. Restoring never runs the CLI; it is read-only. The record is only a cache of the person's own review.
use super::{sql, Provider, Result, SessionManager, Tool};
use rusqlite::params;
use serde::Serialize;
use std::fs;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS reviewed_tools (
    provider TEXT PRIMARY KEY CHECK(provider IN ('claude','codex')), executable TEXT NOT NULL, version TEXT NOT NULL,
    identity TEXT NOT NULL, sha256 TEXT NOT NULL, name_flag INTEGER NOT NULL DEFAULT 0, remote_flag INTEGER NOT NULL DEFAULT 0,
    inspected_at INTEGER NOT NULL DEFAULT 0);";

/// Largest CLI file the app will hash to verify a remembered inspection (bigger ones are simply asked about again).
pub const MAX_HASH_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct RememberedTool { pub provider: Provider, pub executable: PathBuf, pub version: String, identity: String, sha256: String, name_flag: bool, remote_flag: bool }

/// What the page shows per provider: inspected in this run, restored from a remembered inspection, or changed since.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ToolStatus { pub provider: Provider, pub executable: String, pub version: String, pub remembered: bool, pub changed: Option<String> }

/// The identity the sessions store compares (device, inode, size, modification time). Never follows the CLI.
pub fn tool_identity(path: &Path) -> Result<String> { super::executable_identity(path) }

/// SHA-256 of a regular, non-symlink file, read with O_NOFOLLOW and capped at MAX_HASH_BYTES. Run it off the UI thread.
pub fn file_sha256(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut f = fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(path).map_err(|_| "Cannot open the CLI file to verify it")?;
    let m = f.metadata().map_err(|_| "Cannot inspect the CLI file")?;
    if !m.is_file() { return Err("The CLI path is not a regular file".into()); }
    if m.len() > MAX_HASH_BYTES { return Err("The CLI file is too large to verify".into()); }
    let mut h = Sha256::new(); let mut buf = vec![0u8; 1 << 16]; let mut total = 0u64;
    loop {
        let n = f.read(&mut buf).map_err(|_| "Cannot read the CLI file")?;
        if n == 0 { break; }
        total += n as u64; if total > MAX_HASH_BYTES { return Err("The CLI file is too large to verify".into()); }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Cheap checks (no hashing): the path is still the same canonical, regular, executable file, not writable by group or
/// others, with the same identity. Returns the reason when it is not.
pub fn quick_check(r: &RememberedTool) -> std::result::Result<(), String> {
    let lm = fs::symlink_metadata(&r.executable).map_err(|_| "no longer found".to_string())?;
    if lm.file_type().is_symlink() { return Err("the path is now a link".into()); }
    if fs::canonicalize(&r.executable).ok().as_deref() != Some(r.executable.as_path()) { return Err("the path now resolves elsewhere".into()); }
    if !lm.is_file() || lm.mode() & 0o111 == 0 { return Err("not an executable file any more".into()); }
    if lm.mode() & 0o022 != 0 { return Err("the file is writable by others".into()); }
    safe_ancestors(&r.executable)?;
    if tool_identity(&r.executable).ok().as_deref() != Some(r.identity.as_str()) { return Err("the file changed".into()); }
    Ok(())
}

/// The folder holding the CLI must not be writable by group or others (someone could swap the file); every folder
/// above it must be owned by root or by you and not writable by group or others unless it has the sticky bit (as
/// /nix/store and /tmp do, where others cannot replace your entries). Owner-writable folders and files are allowed:
/// that is normal for a CLI installed in your own home.
fn safe_ancestors(path: &Path) -> std::result::Result<(), String> {
    let me = super::sys::uid();
    let parent = path.parent().ok_or("the path has no folder")?;
    let pm = fs::symlink_metadata(parent).map_err(|_| "its folder is missing".to_string())?;
    if !pm.is_dir() || pm.mode() & 0o022 != 0 { return Err("its folder is writable by others".into()); }
    for dir in parent.ancestors() {
        let m = fs::symlink_metadata(dir).map_err(|_| "a parent folder is missing".to_string())?;
        if !m.is_dir() { return Err("a parent folder is a link".into()); }
        if m.uid() != 0 && m.uid() != me { return Err("a parent folder belongs to another user".into()); }
        if m.mode() & 0o022 != 0 && m.mode() & 0o1000 == 0 { return Err("a parent folder is writable by others".into()); }
    }
    Ok(())
}

/// What to re-hash right before a restored tool is used to spawn a session.
#[derive(Clone, Debug)]
pub struct RestoredTarget { pub provider: Provider, pub executable: PathBuf, pub identity: String, pub sha256: String }

impl SessionManager {
    /// If this session would start with a tool restored from a remembered inspection, what to verify first.
    pub fn restored_target(&self, id: &str) -> Result<Option<RestoredTarget>> {
        let info = self.get(id)?;
        Ok(self.tools.get(&info.provider).and_then(|t| t.remembered_sha.as_ref().map(|sha| RestoredTarget {
            provider: info.provider, executable: t.executable.clone(), identity: t.identity.clone(), sha256: sha.clone() })))
    }
    /// Drops a restored tool from this run (its file no longer matches); the person is asked to inspect again.
    pub fn drop_restored(&mut self, provider: Provider) {
        if self.tools.get(&provider).is_some_and(|t| t.remembered_sha.is_some()) { self.tools.remove(&provider); }
    }
    /// Forget: deletes the remembered inspection for this provider and, if this run uses the restored copy, drops it.
    pub fn forget_tool(&mut self, provider: Provider) -> Result<bool> {
        let n = sql(self.connection.execute("DELETE FROM reviewed_tools WHERE provider=?1", params![provider.name()]))?;
        self.drop_restored(provider);
        Ok(n > 0)
    }
    /// Called right after the person's own inspection: stores what was reviewed. `identity` and `sha256` were taken
    /// outside the manager lock; the record is kept only if the configured tool still has that identity.
    pub fn remember_tool(&mut self, provider: Provider, identity: &str, sha256: &str) -> Result<bool> {
        let Some(t) = self.tools.get(&provider) else { return Ok(false) };
        if t.identity != identity || sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) { return Ok(false); }
        let exe = t.executable.to_str().ok_or("CLI path is not valid text")?.to_string();
        sql(self.connection.execute("INSERT INTO reviewed_tools(provider,executable,version,identity,sha256,name_flag,remote_flag,inspected_at)
            VALUES (?1,?2,?3,?4,?5,?6,?7,strftime('%s','now')) ON CONFLICT(provider) DO UPDATE SET executable=excluded.executable,
            version=excluded.version,identity=excluded.identity,sha256=excluded.sha256,name_flag=excluded.name_flag,
            remote_flag=excluded.remote_flag,inspected_at=excluded.inspected_at",
            params![provider.name(), exe, t.version, t.identity, sha256, t.name_flag as i64, t.remote_flag as i64]))?;
        Ok(true)
    }

    /// Remembered inspections for providers that are not configured in this run (nothing to do otherwise).
    pub fn remembered_pending(&self) -> Result<Vec<RememberedTool>> {
        let mut st = sql(self.connection.prepare("SELECT provider,executable,version,identity,sha256,name_flag,remote_flag FROM reviewed_tools"))?;
        let rows = sql(st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?,
            r.get::<_, String>(4)?, r.get::<_, i64>(5)?, r.get::<_, i64>(6)?))))?;
        let mut out = Vec::new();
        for row in rows {
            let (p, exe, version, identity, sha256, nf, rf) = sql(row)?;
            let Ok(provider) = Provider::parse(&p) else { continue };
            if self.tools.contains_key(&provider) { continue; }
            out.push(RememberedTool { provider, executable: PathBuf::from(exe), version, identity, sha256, name_flag: nf != 0, remote_flag: rf != 0 });
        }
        Ok(out)
    }

    /// Restores one remembered inspection after `sha_now` was computed outside the lock. Every check runs again here;
    /// any difference refuses and the person is asked to inspect.
    pub fn adopt_remembered(&mut self, r: &RememberedTool, sha_now: &str) -> std::result::Result<(), String> {
        if self.tools.contains_key(&r.provider) { return Ok(()); }
        quick_check(r)?;
        if sha_now != r.sha256 { return Err("the file contents changed".into()); }
        self.tools.insert(r.provider, Tool { executable: r.executable.clone(), version: r.version.clone(), identity: r.identity.clone(),
            name_flag: r.name_flag, remote_flag: r.remote_flag, remembered_sha: Some(r.sha256.clone()) });
        Ok(())
    }

    /// The tools configured in this run, for the page.
    pub fn tool_statuses(&self) -> Vec<ToolStatus> {
        let mut v: Vec<ToolStatus> = self.tools.iter().map(|(p, t)| ToolStatus { provider: *p, executable: t.executable.to_string_lossy().into_owned(),
            version: t.version.clone(), remembered: t.remembered_sha.is_some(), changed: None }).collect();
        v.sort_by_key(|t| t.provider.name());
        v
    }
}
