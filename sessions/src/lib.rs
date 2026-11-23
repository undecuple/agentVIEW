#![deny(unsafe_code)]
//! Explicitly launched interactive Claude sessions. Metadata is durable; PTY
//! output is bounded memory only. No automatic launch, input replay or fallback.
#[cfg(not(target_os = "linux"))]
compile_error!("Interactive sessions currently require Linux PTYs");
#[allow(unsafe_code)]
mod sys;

use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use uuid::Uuid;
mod codex;
pub mod usage;
pub mod activity;
pub use activity::Activity;
pub use usage::{UsageDay, UsageRoots, UsageSession, UsageSnapshot, UsageTotals};

pub type Result<T> = std::result::Result<T, String>;
const RECIPE: &str = "claude-interactive-v1";
const CODEX_RECIPE: &str = "codex-native-resume-v1";
const MAX_SESSIONS: i64 = 64;
const MAX_ACTIVE: usize = 2;
const RING_BYTES: usize = 256 * 1024;
const READ_BYTES: usize = 64 * 1024;
const WRITE_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider { Claude, Codex }
impl Provider {
    pub fn parse(value: &str) -> Result<Self> {
        match value { "claude" => Ok(Self::Claude), "codex" => Ok(Self::Codex), _ => Err("Unknown live provider".into()) }
    }
    fn name(self) -> &'static str { match self { Self::Claude => "claude", Self::Codex => "codex" } }
    fn recipe(self) -> &'static str { match self { Self::Claude => RECIPE, Self::Codex => CODEX_RECIPE } }
}

#[derive(Clone)]
struct Tool { executable: PathBuf, version: String, identity: String, name_flag: bool, remote_flag: bool }

#[derive(Clone, Debug, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub label: String,
    pub project_dir: String,
    pub executable: String,
    pub version: String,
    pub recipe: String,
    pub attempted: bool,
    pub generation: String,
    pub status: String,
    pub executable_identity: String,
    pub provider: Provider,
    pub provider_session_id: Option<String>,
    pub bootstrap_attempted: bool,
    /// Claude only: start this session with Claude Code's own Remote Control. Applies at the next start.
    pub remote_control: bool,
    /// Name shown everywhere and passed to the CLI: project, label, provider.
    pub display_name: String,
    /// How the last run ended, when the app saw it end (for example `exit 1` or `signal 9`). Cleared on the next start.
    pub exit_info: Option<String>,
    pub exit_at: Option<String>,
}

/// `project · label · Provider`. The project is the folder name; it is dropped when the label already contains it.
pub fn display_name(label: &str, project_dir: &str, provider: Provider) -> String {
    let project = Path::new(project_dir).file_name().and_then(|n| n.to_str()).unwrap_or("");
    let who = match provider { Provider::Claude => "Claude", Provider::Codex => "Codex" };
    let name = if project.is_empty() || label.to_lowercase().contains(&project.to_lowercase()) { format!("{label} · {who}") }
        else { format!("{project} · {label} · {who}") };
    name.chars().filter(|c| !c.is_control()).take(80).collect()
}

/// Terminal bytes cross the IPC boundary as a base64 string, not a JSON array of numbers.
fn base64_bytes<S: serde::Serializer>(bytes: &Vec<u8>, s: S) -> std::result::Result<S::Ok, S::Error> {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18 & 63) as usize] as char); out.push(T[(n >> 12 & 63) as usize] as char);
        out.push(if c.len() > 1 { T[(n >> 6 & 63) as usize] as char } else { '=' });
        out.push(if c.len() > 2 { T[(n & 63) as usize] as char } else { '=' });
    }
    s.serialize_str(&out)
}

#[derive(Debug, Serialize)]
pub struct ReadResult {
    pub generation: String,
    #[serde(serialize_with = "base64_bytes")]
    pub data: Vec<u8>,
    pub start_offset: u64,
    pub next_offset: u64,
    pub truncated: bool,
    pub running: bool,
    pub status: String,
}

#[derive(Default)]
struct Ring { bytes: VecDeque<u8>, start: u64, last_output: Option<Instant> }
impl Ring {
    fn push(&mut self, bytes: &[u8]) {
        self.last_output = Some(Instant::now());
        self.bytes.extend(bytes);
        if self.bytes.len() > RING_BYTES {
            let n = self.bytes.len() - RING_BYTES;
            self.bytes.drain(..n);
            self.start += n as u64;
        }
    }
}

struct Running {
    child: Option<Child>,
    master: File,
    ring: Arc<Mutex<Ring>>,
    stop_reader: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
    generation: String,
    input_bytes: u64,
}
impl Running {
    fn stop(&mut self) {
        if let Some(child) = &mut self.child {
            let pid = child.id();
            sys::signal_group(pid, libc::SIGTERM);
            let until = Instant::now() + Duration::from_millis(250);
            while Instant::now() < until {
                if sys::exited(pid).unwrap_or(true) { break; }
                thread::sleep(Duration::from_millis(10));
            }
            // Kill descendants too, even when the leader has already exited.
            // The unreaped child still reserves its PID/PGID here.
            sys::signal_group(pid, libc::SIGKILL);
            let _ = child.wait();
            self.child = None;
        }
        self.stop_reader.store(true, Ordering::Release);
        if let Some(reader) = self.reader.take() { let _ = reader.join(); }
        // A short-lived CLI can exit before the reader thread's next poll.
        // Preserve its final error text after joining, with a bounded drain.
        let mut bytes = [0u8;8192];
        let mut remaining = RING_BYTES;
        while remaining > 0 {
            match self.master.read(&mut bytes[..remaining.min(8192)]) {
                Ok(0) => break,
                Ok(n) => {
                    if let Ok(mut ring) = self.ring.lock() { ring.push(&bytes[..n]); }
                    remaining -= n;
                },
                Err(e) if e.kind()==std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    }
}
impl Drop for Running { fn drop(&mut self) { self.stop(); } }

/// Usage-only handle from [`SessionManager::open_usage_view`].
pub struct UsageView(SessionManager);
impl UsageView {
    pub fn set_cancel_flag(&mut self, flag: Arc<AtomicBool>) { self.0.cancel = flag; }
    pub fn usage_snapshot(&mut self) -> Result<UsageSnapshot> { self.0.usage_snapshot() }
    pub fn refresh_usage(&mut self, roots: &UsageRoots) -> Result<UsageSnapshot> { self.0.refresh_usage(roots) }
}

pub struct SessionManager {
    connection: Connection,
    tools: HashMap<Provider, Tool>,
    cancel: Arc<AtomicBool>,
    running: HashMap<String, Running>,
    // Keep the lease through cleanup. It serializes app-managed writers using
    // this metadata directory, not external provider clients or copied stores.
    _lease: Option<File>,
}

fn sql<T>(result: rusqlite::Result<T>) -> Result<T> {
    result.map_err(|_| "Session metadata operation failed".into())
}
fn private_file(path: &Path) -> Result<File> {
    let file = OpenOptions::new().read(true).write(true).create(true)
        .mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path).map_err(|_| "Cannot open private session metadata".to_string())?;
    let m = file.metadata().map_err(|_| "Cannot inspect session metadata")?;
    if !m.is_file() || m.uid() != sys::uid() || m.mode() & 0o077 != 0 || m.nlink() != 1 {
        return Err("Session metadata must be private, regular, singly linked and owned by you".into());
    }
    Ok(file)
}

/// Paid API or alternate-cloud overrides must be resolved by the user rather
/// than silently inherited. This never reads/logs their values or changes login.
fn validate_provider_environment(provider: Provider) -> Result<()> {
    let names: &[&str] = match provider {
        Provider::Claude => &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL",
            "CLAUDE_CODE_USE_BEDROCK", "CLAUDE_CODE_USE_VERTEX", "CLAUDE_CODE_USE_FOUNDRY"],
        Provider::Codex => &["OPENAI_API_KEY", "CODEX_API_KEY", "OPENAI_BASE_URL", "AZURE_OPENAI_API_KEY"],
    };
    for name in names {
        if std::env::var_os(name).is_some() {
            return Err(format!("Live launch refused: inherited {name} overrides native login; resolve it outside the app"));
        }
    }
    Ok(())
}

fn metadata(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionInfo> {
    let tag: String = row.get(10)?;
    let provider = Provider::parse(&tag).map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(SessionInfo { id: row.get(0)?, label: row.get(1)?, project_dir: row.get(2)?,
        executable: row.get(3)?, version: row.get(4)?, recipe: row.get(5)?,
        attempted: row.get(6)?, generation: row.get(7)?, status: row.get(8)?, executable_identity: row.get(9)?,
        display_name: display_name(&row.get::<_, String>(1)?, &row.get::<_, String>(2)?, provider),
        provider, provider_session_id: row.get(11)?, bootstrap_attempted: row.get(12)?, remote_control: row.get::<_, i64>(13)? != 0,
        exit_info: row.get(14)?, exit_at: row.get(15)? })
}

fn executable_identity(path: &Path) -> Result<String> {
    let m = fs::metadata(path).map_err(|_| "Reviewed executable is unavailable")?;
    if !m.is_file() || m.mode() & 0o111 == 0 { return Err("Reviewed executable is not executable".into()); }
    Ok(format!("{}:{}:{}:{}:{}", m.dev(), m.ino(), m.size(), m.mtime(), m.mtime_nsec()))
}

impl SessionManager {
    /// Caller supplies only the CLI selected by its reviewed native probe;
    /// executable paths and version strings must never come from web content.
    /// No executable is run while opening metadata or restoring records.
    pub fn open(directory: &Path, reviewed_executable: &Path, version: &str) -> Result<Self> {
        let mut manager = Self::open_unconfigured(directory)?;
        manager.configure_provider(Provider::Claude, reviewed_executable, version)?;
        Ok(manager)
    }

    /// Metadata-only restore. No CLI probe, bootstrap, process or input replay.
    pub fn open_unconfigured(directory: &Path) -> Result<Self> {
        if !directory.is_absolute() || directory.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err("Session data directory must be an absolute path without parent components".into());
        }
        // Only the final directory is created: its existing parent must resolve
        // without symlinks before any new path is opened.
        let parent = directory.parent().ok_or("Session directory has no parent")?;
        if fs::canonicalize(parent).map_err(|_| "Session directory parent is unavailable")? != parent {
            return Err("Session storage paths must not contain symlinks".into());
        }
        match fs::DirBuilder::new().mode(0o700).create(directory) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(_) => return Err("Cannot create private session directory".into()),
        }
        let m = fs::symlink_metadata(directory).map_err(|_| "Cannot inspect session directory")?;
        if !m.is_dir() || m.uid() != sys::uid() || m.mode() & 0o077 != 0 {
            return Err("Session directory must be private, owned by you and not a symlink".into());
        }
        let lease = private_file(&directory.join("manager.lock"))?;
        // A concurrent fork briefly inherits CLOEXEC descriptors until exec.
        // Allow that bounded handoff; never steal an active instance's lease.
        let lock_deadline = Instant::now() + Duration::from_millis(100);
        loop {
            match sys::lock(&lease) {
                Ok(()) => break,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < lock_deadline => {
                    thread::sleep(Duration::from_millis(5));
                },
                Err(_) => return Err("Another app instance owns this session store".into()),
            }
        }
        let db_path = directory.join("sessions.sqlite3");
        let _db_file = private_file(&db_path)?;
        let mut connection = sql(Connection::open_with_flags(&db_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW))?;
        sql(connection.busy_timeout(Duration::from_secs(2)))?;
        let tx = sql(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
        let schema: i64 = sql(tx.query_row("PRAGMA user_version", [], |r| r.get(0)))?;
        if schema > 3 { return Err("Session metadata is from a newer app; refusing changes".into()); }
        if schema == 0 {
            sql(tx.execute_batch("CREATE TABLE sessions (
                id TEXT PRIMARY KEY,label TEXT NOT NULL,cwd TEXT NOT NULL,executable TEXT NOT NULL,
                version TEXT NOT NULL,recipe TEXT NOT NULL,attempted INTEGER NOT NULL DEFAULT 0,
                generation TEXT NOT NULL DEFAULT '',status TEXT NOT NULL DEFAULT 'registered',
                executable_identity TEXT NOT NULL);
                PRAGMA user_version=1;"))?;
        }
        if schema <= 1 {
            sql(tx.execute_batch("ALTER TABLE sessions ADD COLUMN provider TEXT NOT NULL DEFAULT 'claude' CHECK(provider IN ('claude','codex'));
                ALTER TABLE sessions ADD COLUMN provider_session_id TEXT;
                ALTER TABLE sessions ADD COLUMN bootstrap_attempted INTEGER NOT NULL DEFAULT 0;
                UPDATE sessions SET provider_session_id=id;
                CREATE UNIQUE INDEX provider_identity ON sessions(provider,provider_session_id) WHERE provider_session_id IS NOT NULL;
                PRAGMA user_version=2;"))?;
        }
        usage::initialize(&tx)?;
        let has_remote: i64 = sql(tx.query_row("SELECT count(*) FROM pragma_table_info('sessions') WHERE name='remote_control'", [], |r| r.get(0)))?;
        if has_remote == 0 { sql(tx.execute_batch("ALTER TABLE sessions ADD COLUMN remote_control INTEGER NOT NULL DEFAULT 0 CHECK(remote_control IN (0,1));"))?; }
        let has_exit: i64 = sql(tx.query_row("SELECT count(*) FROM pragma_table_info('sessions') WHERE name='exit_info'", [], |r| r.get(0)))?;
        if has_exit == 0 { sql(tx.execute_batch("ALTER TABLE sessions ADD COLUMN exit_info TEXT; ALTER TABLE sessions ADD COLUMN exit_at TEXT;"))?; }
        // Recovery is metadata-only. Never relaunch or resend terminal input.
        sql(tx.execute("UPDATE sessions SET status='interrupted' WHERE status IN ('running','launching')", []))?;
        sql(tx.execute("UPDATE sessions SET status='bootstrap_unknown' WHERE status='bootstrapping'", []))?;
        sql(tx.commit())?;
        Ok(Self { connection, tools: HashMap::new(), cancel: Arc::new(AtomicBool::new(false)), running: HashMap::new(), _lease: Some(lease) })
    }

    /// A second, read-mostly view of an EXISTING session store for usage reading only. It takes no instance lease,
    /// runs no migration or recovery, owns no processes and exposes only usage methods, so a long usage refresh
    /// never waits on (or blocks) the main manager that serves terminals.
    pub fn open_usage_view(directory: &Path) -> Result<UsageView> {
        let m = fs::symlink_metadata(directory).map_err(|_| "Cannot inspect session directory")?;
        if !m.is_dir() || m.uid() != sys::uid() || m.mode() & 0o077 != 0 { return Err("Session directory must be private, owned by you and not a symlink".into()); }
        let db_path = directory.join("sessions.sqlite3");
        let _db_file = private_file(&db_path)?;
        let connection = sql(Connection::open_with_flags(&db_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW))?;
        sql(connection.busy_timeout(Duration::from_secs(2)))?;
        Ok(UsageView(Self { connection, tools: HashMap::new(), cancel: Arc::new(AtomicBool::new(false)), running: HashMap::new(), _lease: None }))
    }

    /// The window owner can cancel an in-progress bootstrap without acquiring
    /// the manager mutex. This method never clears a pending close request.
    pub fn set_cancel_flag(&mut self, flag: Arc<AtomicBool>) { self.cancel = flag; }

    /// Only the trusted native probe can configure tools; web payloads must not
    /// supply executable/version. Configuring a provider never executes it.
    pub fn configure_provider(&mut self, provider: Provider, reviewed_executable: &Path, version: &str) -> Result<()> {
        let executable = fs::canonicalize(reviewed_executable).map_err(|_| "Reviewed Claude executable is unavailable")?;
        let em = fs::metadata(&executable).map_err(|_| "Cannot inspect Claude executable")?;
        if !em.is_file() || em.mode() & 0o111 == 0 || version.trim().is_empty() || version.len() > 200 {
            return Err("Claude executable/version probe is invalid".into());
        }
        let executable_identity = executable_identity(&executable)?;
        self.tools.insert(provider, Tool { executable, version: version.into(), identity: executable_identity, name_flag: false, remote_flag: false });
        Ok(())
    }
    /// Set by the trusted native probe from the CLI's own `--help`; never by web content.
    pub fn set_capabilities(&mut self, provider: Provider, name_flag: bool, remote_flag: bool) {
        if let Some(t) = self.tools.get_mut(&provider) { t.name_flag = name_flag; t.remote_flag = remote_flag; }
    }

    pub fn create(&mut self, label: &str, project_dir: &Path) -> Result<SessionInfo> {
        self.create_for(Provider::Claude, label, project_dir)
    }
    pub fn create_for(&mut self, provider: Provider, label: &str, project_dir: &Path) -> Result<SessionInfo> {
        let tool = self.tools.get(&provider).ok_or("Inspect this provider's installed CLI before registering a session")?;
        if label.trim().is_empty() || label.len() > 100 || label.chars().any(char::is_control) {
            return Err("Session label must be 1–100 bytes without control characters".into());
        }
        let cwd = fs::canonicalize(project_dir).map_err(|_| "Project directory is unavailable")?;
        if !cwd.is_dir() || cwd.to_str().is_none() { return Err("Project must be a UTF-8 directory".into()); }
        let count: i64 = sql(self.connection.query_row("SELECT count(*) FROM sessions", [], |r| r.get(0)))?;
        if count >= MAX_SESSIONS { return Err("Session record limit reached (64)".into()); }
        let id = Uuid::new_v4().to_string();
        let native_id = (provider == Provider::Claude).then_some(id.clone());
        sql(self.connection.execute("INSERT INTO sessions(id,label,cwd,executable,version,recipe,executable_identity,provider,provider_session_id) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![id, label.trim(), cwd.to_str(), tool.executable.to_str().ok_or("Executable path is not UTF-8")?, tool.version, provider.recipe(), tool.identity, provider.name(), native_id]))?;
        self.get(&id)
    }

    fn get(&self, id: &str) -> Result<SessionInfo> {
        sql(self.connection.query_row("SELECT * FROM sessions WHERE id=?1", [id], metadata).optional())?
            .ok_or_else(|| "Unknown session".into())
    }
    /// Remote Control applies at the next start, so it can change only while the session is not running.
    pub fn set_remote_control(&mut self, id: &str, enabled: bool) -> Result<SessionInfo> {
        self.refresh()?;
        let info = self.get(id)?;
        if info.provider != Provider::Claude { return Err("Remote Control is available for Claude Code sessions only".into()); }
        if self.running.get(id).is_some_and(|r| r.child.is_some()) { return Err("Stop the session before changing Remote Control".into()); }
        sql(self.connection.execute("UPDATE sessions SET remote_control=?1 WHERE id=?2", params![enabled as i64, id]))?;
        self.get(id)
    }
    /// Renames the saved label. Only while stopped, because the name is given to the CLI at launch.
    pub fn rename(&mut self, id: &str, label: &str) -> Result<SessionInfo> {
        self.refresh()?;
        self.get(id)?;
        if label.trim().is_empty() || label.len() > 100 || label.chars().any(char::is_control) {
            return Err("Session label must be 1–100 bytes without control characters".into());
        }
        if self.running.get(id).is_some_and(|r| r.child.is_some()) { return Err("Stop the session before renaming it".into()); }
        sql(self.connection.execute("UPDATE sessions SET label=?1 WHERE id=?2", params![label.trim(), id]))?;
        self.get(id)
    }
    fn generation(&self, id: &str, generation: &str) -> Result<SessionInfo> {
        let info = self.get(id)?;
        if info.generation != generation { return Err("Session launch changed; refresh before acting".into()); }
        Ok(info)
    }
    fn refresh(&mut self) -> Result<()> {
        for (id, run) in &mut self.running {
            if let Some(child) = &run.child {
                if sys::exited(child.id()).map_err(|_| "Cannot inspect owned session process")? {
                    use std::os::unix::process::ExitStatusExt;
                    let info = run.child.as_mut().and_then(|c| c.try_wait().ok().flatten()).map(|s| match (s.code(), s.signal()) {
                        (Some(c), _) => format!("exit {c}"), (None, Some(g)) => format!("signal {g}"), _ => "ended".to_string() });
                    run.stop();
                    sql(self.connection.execute("UPDATE sessions SET status='exited',exit_info=?2,exit_at=strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id=?1", params![id, info]))?;
                }
            }
        }
        Ok(())
    }
    pub fn list(&mut self) -> Result<Vec<SessionInfo>> {
        self.refresh()?;
        let mut statement = sql(self.connection.prepare("SELECT * FROM sessions ORDER BY rowid LIMIT 65"))?;
        let result: Vec<SessionInfo> = sql(sql(statement.query_map([], metadata))?.collect())?;
        if result.len() > MAX_SESSIONS as usize { return Err("Session record limit exceeded".into()); }
        Ok(result)
    }

    pub fn start(&mut self, id: &str) -> Result<SessionInfo> {
        self.refresh()?;
        if self.cancel.load(Ordering::Acquire) { return Err("Session launch cancelled by workspace close".into()); }
        if self.running.get(id).is_some_and(|r| r.child.is_some()) { return Err("Session is already running".into()); }
        if self.running.values().filter(|r| r.child.is_some()).count() >= MAX_ACTIVE { return Err("Two live sessions are already active".into()); }
        let mut info = self.get(id)?;
        validate_provider_environment(info.provider)?;
        let tool = self.tools.get(&info.provider).cloned().ok_or("Inspect this provider's installed CLI before starting its session")?;
        if Path::new(&info.executable) != tool.executable || info.version != tool.version || info.recipe != info.provider.recipe()
            || info.executable_identity != tool.identity {
            return Err("Saved CLI executable/version/recipe changed; review and register a new session before launch".into());
        }
        if fs::canonicalize(&info.project_dir).map_err(|_| "Saved project directory is unavailable")? != Path::new(&info.project_dir) {
            return Err("Saved project directory changed; refusing launch".into());
        }
        if fs::canonicalize(&tool.executable).map_err(|_| "Reviewed executable is unavailable")? != tool.executable
            || executable_identity(&tool.executable)? != tool.identity {
            return Err("Reviewed executable changed; repeat the CLI probe".into());
        }
        Uuid::parse_str(id).map_err(|_| "Invalid saved provider session ID")?;
        let generation = Uuid::new_v4().to_string();
        if info.provider == Provider::Codex && info.provider_session_id.is_none() {
            if info.bootstrap_attempted {
                return Err("Codex identity creation was previously attempted but its outcome is unknown; inspect native provider history. This record will not create a fallback thread".into());
            }
            sql(self.connection.execute("UPDATE sessions SET bootstrap_attempted=1,generation=?1,status='bootstrapping' WHERE id=?2", params![generation,id]))?;
            match codex::bootstrap(&tool.executable, &info.project_dir, &info.label, Arc::clone(&self.cancel)) {
                Ok(native) => {
                    let saved=(||->Result<()>{
                        let tx=sql(self.connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
                        sql(tx.execute("UPDATE sessions SET provider_session_id=?1,status='registered' WHERE id=?2",params![native.id,id]))?;
                        usage::bind_codex(&tx,id,&native.path)?;
                        sql(tx.commit())
                    })();
                    if saved.is_err() {
                        let _ = self.connection.execute("UPDATE sessions SET status='bootstrap_unknown' WHERE id=?1",[id]);
                        return Err("Codex identity could not be saved; outcome unknown, no fallback thread will be created".into());
                    }
                    info = self.get(id)?;
                },
                Err(error) => {
                    let _ = self.connection.execute("UPDATE sessions SET status='bootstrap_unknown' WHERE id=?1",[id]);
                    return Err(error);
                }
            }
        }
        let native_id = info.provider_session_id.as_deref().ok_or("Saved native provider identity is missing")?;
        codex::validate_uuid(native_id)?;
        if info.provider == Provider::Claude && native_id != id { return Err("Claude native identity does not match its saved app record".into()); }
        if self.cancel.load(Ordering::Acquire) { return Err("Session launch cancelled by workspace close".into()); }
        // An uncertain first launch must always recover by exact resume, never
        // make a second conversation or replay input after app/exec failure.
        sql(self.connection.execute("UPDATE sessions SET attempted=1,generation=?1,status='launching',exit_info=NULL,exit_at=NULL WHERE id=?2", params![generation,id]))?;
        let mut command = Command::new(&tool.executable);
        command.current_dir(&info.project_dir).env("TERM", "xterm-256color").env("COLORTERM", "truecolor");
        match info.provider {
            Provider::Claude => {
                command.arg(if info.attempted { "--resume" } else { "--session-id" }).arg(native_id);
                // Name flags are passed only when this CLI lists them, so an older CLI still launches.
                if tool.name_flag { command.arg("--name").arg(&info.display_name); }
                if info.remote_control {
                    if !tool.remote_flag { return Err("This Claude Code CLI does not offer Remote Control; update it or turn the option off".into()); }
                    command.arg("--remote-control").arg(&info.display_name);
                }
            },
            Provider::Codex => { command.arg("resume").arg(native_id).arg("--no-daemon"); },
        }
        let launched = (|| -> std::io::Result<Running> {
            let master = sys::pty(&mut command)?;
            let mut reader_file = master.try_clone()?;
            let ring = Arc::new(Mutex::new(Ring::default()));
            let stop_reader = Arc::new(AtomicBool::new(false));
            let buffer = Arc::clone(&ring);
            let stop = Arc::clone(&stop_reader);
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            // PDEATHSIG belongs to the exact thread that forks. Spawn on the
            // persistent PTY thread, never a transient Tauri dispatch thread.
            let reader = thread::Builder::new().name("session-pty".into()).spawn(move || {
                match command.spawn() {
                    Ok(child) => {
                        if let Err(error) = sender.send(Ok(child)) {
                            if let Ok(mut child) = error.0 {
                                sys::signal_group(child.id(), libc::SIGKILL);
                                let _ = child.wait();
                            }
                            return;
                        }
                    },
                    Err(error) => { let _ = sender.send(Err(error)); return; },
                }
                let mut bytes = [0u8; 8192];
                let mut eof = false;
                loop {
                    // Keep this parent thread alive through explicit cleanup,
                    // including when a live CLI closes its terminal descriptors.
                    if eof {
                        if stop.load(Ordering::Acquire) { break; }
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    match reader_file.read(&mut bytes) {
                        Ok(0) => eof = true,
                        Ok(n) => { if let Ok(mut ring) = buffer.lock() { ring.push(&bytes[..n]); } else { break; } },
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            if stop.load(Ordering::Acquire) { break; }
                            thread::sleep(Duration::from_millis(10));
                        },
                        Err(_) => eof = true, // PTY EIO is the normal slave-close EOF.
                    }
                    if stop.load(Ordering::Acquire) { break; }
                }
            });
            let reader = reader?;
            match receiver.recv() {
                Ok(Ok(child)) => Ok(Running { child: Some(child), master, ring, stop_reader,
                    reader: Some(reader), generation: generation.clone(), input_bytes: 0 }),
                result => {
                    stop_reader.store(true, Ordering::Release);
                    let _ = reader.join();
                    Err(match result {
                        Ok(Err(error)) => error,
                        _ => std::io::Error::other("PTY launch thread failed"),
                    })
                }
            }
        })();
        match launched {
            Ok(run) => {
                self.running.insert(id.into(), run);
                if self.connection.execute("UPDATE sessions SET status='running' WHERE id=?1", [id]).is_err() {
                    if let Some(run) = self.running.get_mut(id) { run.stop(); }
                    let _ = self.connection.execute("UPDATE sessions SET status='launch_failed' WHERE id=?1", [id]);
                    return Err("Native CLI started but its state could not be saved; the owned process was stopped. No input was replayed. Inspect before an exact-ID resume".into());
                }
                self.get(id)
            },
            Err(_) => {
                sql(self.connection.execute("UPDATE sessions SET status='launch_failed' WHERE id=?1", [id]))?;
                Err("Native CLI launch failed; no input was replayed. The next explicit attempt uses the exact saved ID with Claude --resume or Codex resume; it will not create a fallback conversation".into())
            },
        }
    }

    pub fn read(&mut self, id: &str, generation: &str, offset: u64) -> Result<ReadResult> {
        self.refresh()?;
        let info = self.generation(id, generation)?;
        let Some(run) = self.running.get(id) else {
            return Ok(ReadResult { generation: info.generation, data: Vec::new(), start_offset: 0,
                next_offset: 0, truncated: offset > 0, running: false, status: info.status });
        };
        let ring = run.ring.lock().map_err(|_| "Terminal output is unavailable")?;
        let end = ring.start + ring.bytes.len() as u64;
        if offset > end { return Err("Terminal offset is ahead of this launch; refresh".into()); }
        let start = offset.max(ring.start);
        if start == end && offset >= ring.start {
            return Ok(ReadResult { generation: run.generation.clone(), next_offset: end, data: Vec::new(), start_offset: start,
                truncated: false, running: run.child.is_some(), status: info.status });
        }
        let data: Vec<u8> = ring.bytes.iter().skip((start-ring.start) as usize).take(READ_BYTES).copied().collect();
        Ok(ReadResult { generation: run.generation.clone(), next_offset: start + data.len() as u64,
            data, start_offset: start, truncated: offset < ring.start,
            running: run.child.is_some(), status: info.status })
    }
    pub fn write(&mut self, id: &str, generation: &str, bytes: &[u8]) -> Result<usize> {
        self.refresh()?;
        self.generation(id, generation)?;
        if bytes.is_empty() || bytes.len() > WRITE_BYTES { return Err("Terminal input must be 1–16384 bytes".into()); }
        let run = self.running.get_mut(id).filter(|r| r.child.is_some()).ok_or("Session is not running")?;
        let written = run.master.write(bytes).map_err(|_| "Terminal input was not accepted; delivery may be partial, do not replay automatically".to_string())?;
        run.input_bytes += written as u64;
        Ok(written)
    }
    pub fn resize(&mut self, id: &str, generation: &str, cols: u16, rows: u16) -> Result<()> {
        self.refresh()?;
        self.generation(id, generation)?;
        if !(10..=500).contains(&cols) || !(5..=300).contains(&rows) { return Err("Terminal size is outside supported bounds".into()); }
        let run = self.running.get(id).filter(|r| r.child.is_some()).ok_or("Session is not running")?;
        sys::resize(&run.master, cols, rows).map_err(|_| "Cannot resize session terminal".into())
    }
    pub fn stop(&mut self, id: &str, generation: &str) -> Result<SessionInfo> {
        self.generation(id, generation)?;
        if let Some(run) = self.running.get_mut(id) { run.stop(); }
        sql(self.connection.execute("UPDATE sessions SET status='stopped' WHERE id=?1", [id]))?;
        self.get(id)
    }
    /// Stops every process this app started and returns how many were running. Marks them stopped (not exited).
    pub fn stop_all(&mut self) -> Result<usize> {
        let ids: Vec<String> = self.running.iter().filter(|(_, r)| r.child.is_some()).map(|(id, _)| id.clone()).collect();
        for id in &ids {
            if let Some(run) = self.running.get_mut(id) { run.stop(); }
            sql(self.connection.execute("UPDATE sessions SET status='stopped',exit_info=NULL,exit_at=NULL WHERE id=?1", [id]))?;
        }
        Ok(ids.len())
    }
    pub fn shutdown(&mut self) -> Result<()> {
        for run in self.running.values_mut() { run.stop(); }
        let mut result = Ok(());
        for id in self.running.keys() {
            if let Err(error) = sql(self.connection.execute("UPDATE sessions SET status='stopped' WHERE id=?1", [id])) {
                result = Err(error);
            }
        }
        result
    }
}
impl Drop for SessionManager {
    fn drop(&mut self) {
        // Kill every owned process even when one SQLite update fails.
        for run in self.running.values_mut() { run.stop(); }
        let _ = self.connection.execute("UPDATE sessions SET status='interrupted' WHERE status IN ('running','launching')", []);
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod codex_tests;
#[cfg(test)]
mod usage_tests;
