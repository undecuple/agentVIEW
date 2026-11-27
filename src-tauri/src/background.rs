//! Run sessions in the background. With the preference "background", closing the workspace window hides it while
//! sessions keep running in this process. A second launch (or `agentview --quit`) talks to the first one over a
//! private unix socket in the app data directory. No tray icon, no network, no new dependency.
//!
//! The socket protocol is two words, `show` and `quit`, one per connection. Nothing in a message is ever executed,
//! passed to a shell or used as a path. The reply is `ok`.
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

pub const SOCKET_NAME: &str = "control.sock";
/// A command line is at most this long, newline included. Anything longer is refused unread.
pub const MAX_LINE: usize = 16;
const IO_TIMEOUT: Duration = Duration::from_secs(2);
/// At most this many connections are handled at once; further ones are dropped unanswered.
const MAX_CONNECTIONS: usize = 4;
/// This many accept errors in a row end the serve loop (and so the background close).
const MAX_ACCEPT_ERRORS: u32 = 5;
pub const LOCK_NAME: &str = "control.lock";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command { Show, Quit }

impl Command {
    fn word(self) -> &'static str { match self { Command::Show => "show", Command::Quit => "quit" } }
}

/// Exactly the two known words, nothing else (no trimming of other bytes, no case folding).
pub fn parse_line(line: &[u8]) -> Option<Command> {
    match line { b"show" => Some(Command::Show), b"quit" => Some(Command::Quit), _ => None }
}

/// Reads one line of at most MAX_LINE bytes (the newline included). End of input also ends the line.
pub fn read_line(mut input: impl Read) -> Result<Vec<u8>, &'static str> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    let began = std::time::Instant::now();
    loop {
        // One overall deadline per line: a peer that drips a byte at a time cannot hold a handler slot past it.
        if began.elapsed() > IO_TIMEOUT { return Err("line too slow"); }
        match input.read(&mut byte) {
            Ok(0) => return Ok(line),
            Ok(_) if byte[0] == b'\n' => return Ok(line),
            Ok(_) => { if line.len() + 1 >= MAX_LINE { return Err("line too long"); } line.push(byte[0]); }
            Err(_) => return Err("read failed"),
        }
    }
}

/// The directory must be a real directory owned by this user and closed to everyone else.
pub fn check_dir(dir: &Path) -> Result<(), String> {
    let m = std::fs::symlink_metadata(dir).map_err(|_| "Cannot inspect the control directory")?;
    if !m.is_dir() || m.uid() != agent_app_core::sys::euid() || m.mode() & 0o077 != 0 {
        return Err("The control directory must be private and owned by you".into());
    }
    Ok(())
}

pub enum Claim {
    /// This process owns the socket and should serve it.
    Mine(UnixListener),
    /// Another instance answered on the socket.
    Other,
}

/// Holds an exclusive flock on `control.lock` (private file in the 0700 directory) so two simultaneous starters
/// cannot both see a stale socket and replace it. The lock is released when this value is dropped.
struct ClaimLock(#[allow(dead_code)] std::fs::File);

impl ClaimLock {
    fn take(dir: &Path) -> Result<Self, String> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new().read(true).write(true).create(true).mode(0o600).custom_flags(libc::O_NOFOLLOW)
            .open(dir.join(LOCK_NAME)).map_err(|_| "Cannot open the control lock")?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 { return Ok(ClaimLock(file)); }
            if Instant::now() >= deadline { return Err("The control lock is busy".into()); }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Decides who runs: connects to an existing socket (a live instance answers), replaces a stale one
/// (connection refused) and otherwise binds a new one. A path that is not a socket of this user is refused.
/// The whole decision runs under the control lock, so simultaneous starters are serialised.
pub fn claim(dir: &Path) -> Result<Claim, String> {
    check_dir(dir)?;
    let _lock = ClaimLock::take(dir)?;
    let path = dir.join(SOCKET_NAME);
    for _ in 0..3 {
        match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("Cannot inspect the control socket".into()),
            Ok(m) => {
                if m.file_type().is_symlink() || !m.file_type().is_socket() || m.uid() != agent_app_core::sys::euid() {
                    return Err("The control socket path is not a socket owned by you; not touching it".into());
                }
                match UnixStream::connect(&path) {
                    Ok(_) => return Ok(Claim::Other),
                    Err(e) if matches!(e.kind(), std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound) => {
                        // Nobody listens: left behind by a crashed or killed instance.
                        match std::fs::remove_file(&path) {
                            Ok(()) => {}
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                            Err(_) => return Err("Cannot remove a stale control socket".into()),
                        }
                    }
                    Err(_) => return Err("Cannot reach the control socket".into()),
                }
            }
        }
        match UnixListener::bind(&path) {
            Ok(listener) => {
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(|_| "Cannot restrict the control socket")?;
                return Ok(Claim::Mine(listener));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => continue,
            Err(_) => return Err("Cannot create the control socket".into()),
        }
    }
    Err("The control socket is busy".into())
}

#[derive(Debug, PartialEq, Eq)]
pub enum SendError { NotRunning, Failed(String) }

/// Sends one command to the instance that owns the socket in `dir` and waits for `ok`.
pub fn send(dir: &Path, command: Command) -> Result<(), SendError> {
    check_dir(dir).map_err(SendError::Failed)?;
    let path = dir.join(SOCKET_NAME);
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(SendError::NotRunning),
        Err(_) => return Err(SendError::Failed("Cannot inspect the control socket".into())),
        Ok(m) => if m.file_type().is_symlink() || !m.file_type().is_socket() || m.uid() != agent_app_core::sys::euid() {
            return Err(SendError::Failed("The control socket path is not a socket owned by you".into()));
        },
    }
    let mut stream = match UnixStream::connect(&path) {
        Ok(s) => s,
        Err(e) if matches!(e.kind(), std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound) => return Err(SendError::NotRunning),
        Err(_) => return Err(SendError::Failed("Cannot reach the control socket".into())),
    };
    if peer_uid(&stream) != Some(agent_app_core::sys::euid()) {
        return Err(SendError::Failed("The control socket is served by another user; not sending".into()));
    }
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    stream.write_all(format!("{}\n", command.word()).as_bytes()).map_err(|_| SendError::Failed("The running instance did not accept the command".into()))?;
    let reply = read_line(&mut stream).map_err(|_| SendError::Failed("The running instance did not answer".into()))?;
    if reply == b"ok" { Ok(()) } else { Err(SendError::Failed("The running instance refused the command".into())) }
}

/// The uid of the process on the other end, from the kernel (SO_PEERCRED).
pub fn peer_uid(stream: &UnixStream) -> Option<u32> {
    let mut cred = libc::ucred { pid: 0, uid: u32::MAX, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe { libc::getsockopt(stream.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, &mut cred as *mut _ as *mut libc::c_void, &mut len) };
    (rc == 0 && len as usize == std::mem::size_of::<libc::ucred>()).then_some(cred.uid)
}

/// Handles one connection: the peer must be this user, the line short and one of the two words.
pub fn handle(mut stream: UnixStream, uid: u32, on_command: &dyn Fn(Command)) -> Result<Command, &'static str> {
    if peer_uid(&stream) != Some(uid) { return Err("peer is not this user"); }
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let line = read_line(&mut stream)?;
    match parse_line(&line) {
        Some(command) => { let _ = stream.write_all(b"ok\n"); on_command(command); Ok(command) }
        None => { let _ = stream.write_all(b"err\n"); Err("unknown command") }
    }
}

/// Serves connections until the process exits or accepting keeps failing. Each connection runs on its own short-lived
/// thread (bounded by the 2 second timeouts), at most MAX_CONNECTIONS at once; extra connections are dropped.
/// The returned flag is true while the loop runs; it goes false when the loop ends.
pub fn serve(listener: UnixListener, on_command: impl Fn(Command) + Send + Sync + 'static) -> Arc<AtomicBool> {
    let uid = agent_app_core::sys::euid();
    let alive = Arc::new(AtomicBool::new(true));
    let flag = alive.clone();
    let on_command = Arc::new(on_command);
    let active = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        let mut errors = 0u32;
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    errors = 0;
                    if active.fetch_add(1, Ordering::AcqRel) >= MAX_CONNECTIONS { active.fetch_sub(1, Ordering::AcqRel); continue; }
                    let (slot, on_command) = (active.clone(), on_command.clone());
                    let spawned = std::thread::Builder::new().spawn(move || { let _ = handle(stream, uid, &*on_command); slot.fetch_sub(1, Ordering::AcqRel); });
                    if spawned.is_err() { active.fetch_sub(1, Ordering::AcqRel); }
                }
                Err(_) => {
                    errors += 1;
                    if errors >= MAX_ACCEPT_ERRORS { break; }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }
        eprintln!("agentVIEW: the control socket stopped serving; closing the window will quit as usual");
        flag.store(false, Ordering::Release);
    });
    alive
}

/// True when `path` is still the socket we bound (a socket of ours with the same inode) and the serve loop runs.
pub fn control_ok(path: &Path, inode: u64, alive: &AtomicBool) -> bool {
    alive.load(Ordering::Acquire)
        && std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_socket() && !m.file_type().is_symlink() && m.uid() == agent_app_core::sys::euid() && m.ino() == inode)
}

/// Does another process hold the sessions store lease? (Probe: take and release the lock without creating anything.)
pub fn store_in_use(app_data: &Path) -> bool {
    let Ok(file) = std::fs::OpenOptions::new().read(true).write(true).custom_flags_nofollow().open(app_data.join("sessions").join("manager.lock")) else { return false };
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    rc != 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock
}

trait NoFollow { fn custom_flags_nofollow(&mut self) -> &mut Self; }
impl NoFollow for std::fs::OpenOptions {
    fn custom_flags_nofollow(&mut self) -> &mut Self { use std::os::unix::fs::OpenOptionsExt; self.custom_flags(libc::O_NOFOLLOW) }
}

// ---- application side ------------------------------------------------------------------------------------

/// The socket we bound: its path and inode. Set once, right after a successful claim.
static SOCKET: OnceLock<(PathBuf, u64)> = OnceLock::new();
/// True while our serve loop runs. Set when control starts.
static SERVING: OnceLock<Arc<AtomicBool>> = OnceLock::new();
static HIDDEN: AtomicBool = AtomicBool::new(false);
/// Test-only counter the e2e page reads to see that "show" arrived.
#[cfg(feature = "workspace-e2e")]
pub static SHOWN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub enum Startup { Exit(i32), Run(Option<UnixListener>) }

/// Sends with a few retries: a busy, starting or quitting first instance may need a moment.
fn send_retry(dir: &Path, command: Command) -> Result<(), SendError> {
    let mut last = Err(SendError::NotRunning);
    for attempt in 0..3 {
        if attempt > 0 { std::thread::sleep(Duration::from_millis(500)); }
        last = send(dir, command);
        match last { Err(SendError::Failed(_)) => continue, _ => break }
    }
    last
}

/// Runs before any window exists. A second launch hands over to the first instance and exits (in the default
/// mode too); `--quit` asks it to quit. A first instance that owns the socket but does not answer never blocks the
/// start, unless it still holds the sessions store (then starting would fight over it and we stop with a message).
pub fn startup(quit_flag: bool) -> Startup {
    if cfg!(all(feature = "e2e", not(feature = "workspace-e2e"))) { return Startup::Run(None); }
    let dir = match super::app_data_dir() {
        Ok(d) => d,
        Err(e) => { if quit_flag { eprintln!("agentVIEW: {e}"); return Startup::Exit(1); } return Startup::Run(None); }
    };
    if quit_flag {
        return match send(&dir, Command::Quit) {
            Ok(()) => { println!("agentVIEW: asked the running instance to stop its sessions and quit"); Startup::Exit(0) }
            Err(SendError::NotRunning) => { println!("agentVIEW is not running"); Startup::Exit(0) }
            Err(SendError::Failed(e)) => { eprintln!("agentVIEW: {e}"); Startup::Exit(1) }
        };
    }
    match claim(&dir) {
        Ok(Claim::Other) => match send_retry(&dir, Command::Show) {
            Ok(()) => { println!("agentVIEW is already running; its window was brought back"); Startup::Exit(0) }
            Err(failure) => {
                let why = match failure { SendError::Failed(e) => e, SendError::NotRunning => "it went away".to_string() };
                if store_in_use(&dir) {
                    eprintln!("agentVIEW: another agentVIEW instance is running but does not answer ({why}), and it holds the sessions store. Not starting a second one; wait for it, or end that process and start again.");
                    Startup::Exit(1)
                } else {
                    eprintln!("agentVIEW: another instance owns the control socket but does not answer ({why}); starting without background control");
                    Startup::Run(None)
                }
            }
        },
        Ok(Claim::Mine(listener)) => {
            let path = dir.join(SOCKET_NAME);
            match std::fs::symlink_metadata(&path) {
                Ok(m) => { let _ = SOCKET.set((path, m.ino())); Startup::Run(Some(listener)) }
                Err(_) => { eprintln!("agentVIEW: control socket unavailable: cannot inspect it"); Startup::Run(None) }
            }
        }
        // A problem with the control socket never blocks startup; the app just runs without it.
        Err(e) => { eprintln!("agentVIEW: control socket unavailable: {e}"); Startup::Run(None) }
    }
}

/// Removes our socket file on a clean exit (only if it is still the one we bound). A crash leaves it behind; the
/// next start detects that.
pub fn cleanup() {
    if let Some((path, inode)) = SOCKET.get() {
        if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_socket() && m.ino() == *inode) { let _ = std::fs::remove_file(path); }
    }
}

fn windows(app: &tauri::AppHandle) -> Vec<tauri::WebviewWindow> {
    use tauri::Manager;
    ["workspace", "docs"].iter().filter_map(|l| app.get_webview_window(l)).collect()
}

/// Brings the hidden (or minimised, or background) window back.
pub fn show(app: &tauri::AppHandle) {
    HIDDEN.store(false, Ordering::Release);
    for w in windows(app) {
        let _ = w.show();
        let _ = w.unminimize();
        if w.label() == "workspace" { let _ = w.set_focus(); }
    }
    #[cfg(feature = "workspace-e2e")]
    { SHOWN.fetch_add(1, Ordering::SeqCst); eprintln!("WORKSPACE_E2E control=show"); }
}

/// Stops every session, removes the socket and exits. Runs on its own thread so the caller never waits.
/// A failing stop is logged and the app exits anyway. Output of sessions that ended while the window was hidden is
/// not kept: the idle watcher exits the app as soon as the last session has ended.
pub fn quit(app: &tauri::AppHandle, why: &'static str) {
    use tauri::Manager;
    let app = app.clone();
    std::thread::spawn(move || {
        let result = app.state::<super::live::LiveState>().stop_for_exit();
        if let Err(e) = &result { eprintln!("agentVIEW: stopping the sessions failed ({why}): {e}; exiting anyway"); }
        #[cfg(feature = "workspace-e2e")]
        eprintln!("WORKSPACE_E2E control=quit why={why} stopped={}", result.is_ok());
        cleanup();
        app.exit(0);
    });
}

/// True when the preference says "background" and at least one session is running. Never waits on a lock: when
/// the preferences or the session manager are busy the answer is no, which is the normal (stop and quit) close.
pub fn wants_background(app: &tauri::AppHandle) -> bool {
    use tauri::Manager;
    // Hiding is only safe when we can be brought back: our socket must still be in place and served.
    let (Some((path, inode)), Some(alive)) = (SOCKET.get(), SERVING.get()) else { return false };
    if !control_ok(path, *inode, alive) { return false; }
    let background = {
        let state = app.state::<super::WorkspaceState>();
        let Ok(store) = state.0.as_ref() else { return false };
        let Ok(store) = store.try_lock() else { return false };
        store.settings_snapshot().is_ok_and(|s| s.settings.close_behavior == "background")
    };
    background && app.state::<super::live::LiveState>().peek_running().is_some_and(|n| n > 0)
}

/// Hides the window instead of closing it. The app process and its sessions stay.
pub fn hide(app: &tauri::AppHandle) {
    HIDDEN.store(true, Ordering::Release);
    for w in windows(app) { let _ = w.hide(); }
}

/// Watches a hidden app: when its last session ends there is nothing left to keep alive, so it exits.
pub fn start_watcher(app: tauri::AppHandle) {
    use tauri::Manager;
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(2));
        if HIDDEN.load(Ordering::Acquire) && app.state::<super::live::LiveState>().peek_running() == Some(0) {
            quit(&app, "idle");
            return;
        }
    });
}

/// Wires the control socket to the app.
pub fn start_control(app: tauri::AppHandle, listener: UnixListener) {
    #[cfg(feature = "workspace-e2e")]
    eprintln!("WORKSPACE_E2E pid={}", std::process::id());
    let handle = app.clone();
    let alive = serve(listener, move |command| match command {
        Command::Show => show(&app),
        Command::Quit => quit(&app, "control"),
    });
    let _ = SERVING.set(alive);
    watch_signals(handle);
}

static SIGNAL_PIPE: AtomicI32 = AtomicI32::new(-1);

extern "C" fn on_signal(sig: libc::c_int) {
    let fd = SIGNAL_PIPE.load(Ordering::Relaxed);
    if fd >= 0 { let byte = sig as u8; unsafe { libc::write(fd, &byte as *const u8 as *const libc::c_void, 1); } }
}

/// Installs handlers for SIGTERM, SIGHUP and SIGINT that only write the signal number into a pipe (async-signal-safe).
/// Returns the read end. Handlers go back to their default after the first signal, so a second one still kills.
pub fn install_signal_pipe() -> Option<i32> {
    let mut fds = [0 as libc::c_int; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 { return None; }
    SIGNAL_PIPE.store(fds[1], Ordering::Relaxed);
    for sig in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as usize;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(sig, &action, std::ptr::null_mut());
        }
    }
    Some(fds[0])
}

pub fn restore_signal_defaults() {
    for sig in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] { unsafe { libc::signal(sig, libc::SIG_DFL); } }
}

/// A terminating signal takes the same path as the Quit button: stop the sessions, remove the socket, exit.
fn watch_signals(app: tauri::AppHandle) {
    let Some(fd) = install_signal_pipe() else { return };
    std::thread::spawn(move || {
        let mut byte = [0u8; 1];
        loop {
            let n = unsafe { libc::read(fd, byte.as_mut_ptr() as *mut libc::c_void, 1) };
            if n == 1 { break; }
            if n < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted { return; }
        }
        restore_signal_defaults();
        eprintln!("agentVIEW: signal {} received; stopping the sessions and quitting", byte[0]);
        quit(&app, "signal");
    });
}

/// Quit agentVIEW now: stops all sessions and exits, whatever the close preference says.
#[tauri::command]
pub fn app_quit(window: tauri::WebviewWindow, app: tauri::AppHandle) -> Result<(), String> {
    super::authorized(&window)?;
    quit(&app, "button");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::DirBuilderExt;

    fn private_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agentview-bg-{tag}-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')', ' '], ""));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
        dir
    }

    #[test]
    fn only_the_two_words_parse() {
        assert_eq!(parse_line(b"show"), Some(Command::Show));
        assert_eq!(parse_line(b"quit"), Some(Command::Quit));
        for bad in [&b""[..], b"SHOW", b"show ", b" quit", b"quit\r", b"show;rm", b"exit", b"show\0"] { assert_eq!(parse_line(bad), None, "{bad:?}"); }
    }

    #[test]
    fn line_length_is_capped() {
        assert_eq!(read_line(&b"show\n"[..]), Ok(b"show".to_vec()));
        assert_eq!(read_line(&b"quit"[..]), Ok(b"quit".to_vec()));
        assert!(read_line(&[b'a'; 4096][..]).is_err());
        assert!(read_line(&[b'a'; MAX_LINE][..]).is_err());
        // A peer that sends one byte at a time, slowly, hits the overall deadline instead of holding the handler.
        struct Drip(u8);
        impl std::io::Read for Drip { fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> { std::thread::sleep(Duration::from_millis(500)); b[0] = b'a'; self.0 += 1; Ok(1) } }
        assert_eq!(read_line(Drip(0)), Err("line too slow"));
        assert!(read_line(&[b'a'; MAX_LINE - 2][..]).is_ok());
        // Only the first line is ever read, the rest stays unread.
        assert_eq!(read_line(&b"show\nquit\n"[..]), Ok(b"show".to_vec()));
    }

    #[test]
    fn claim_binds_private_socket_and_second_claim_sees_it() {
        let dir = private_dir("claim");
        let Claim::Mine(listener) = claim(&dir).unwrap() else { panic!("expected to own the socket") };
        let mode = std::fs::metadata(dir.join(SOCKET_NAME)).unwrap().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        serve(listener, move |c| log.lock().unwrap().push(c));
        assert!(matches!(claim(&dir).unwrap(), Claim::Other));
        send(&dir, Command::Show).unwrap();
        send(&dir, Command::Quit).unwrap();
        assert_eq!(*seen.lock().unwrap(), vec![Command::Show, Command::Quit]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_socket_is_detected_and_replaced() {
        let dir = private_dir("stale");
        // A socket file nobody listens on: bind, then drop the listener without removing the file.
        drop(UnixListener::bind(dir.join(SOCKET_NAME)).unwrap());
        assert!(std::fs::symlink_metadata(dir.join(SOCKET_NAME)).unwrap().file_type().is_socket());
        assert_eq!(send(&dir, Command::Show), Err(SendError::NotRunning));
        let Claim::Mine(listener) = claim(&dir).unwrap() else { panic!("stale socket must be replaced") };
        serve(listener, |_| {});
        send(&dir, Command::Show).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn send_without_instance_reports_not_running() {
        let dir = private_dir("none");
        assert_eq!(send(&dir, Command::Quit), Err(SendError::NotRunning));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_symlink_and_regular_file_paths() {
        let dir = private_dir("refuse");
        let target = dir.join("target");
        std::fs::write(&target, b"x").unwrap();
        std::os::unix::fs::symlink(&target, dir.join(SOCKET_NAME)).unwrap();
        assert!(claim(&dir).is_err());
        assert!(matches!(send(&dir, Command::Show), Err(SendError::Failed(_))));
        std::fs::remove_file(dir.join(SOCKET_NAME)).unwrap();
        std::fs::write(dir.join(SOCKET_NAME), b"not a socket").unwrap();
        assert!(claim(&dir).is_err());
        assert_eq!(std::fs::read(dir.join(SOCKET_NAME)).unwrap(), b"not a socket", "a foreign file is never removed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_a_directory_open_to_others() {
        let dir = private_dir("open");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(claim(&dir).is_err());
        assert!(matches!(send(&dir, Command::Show), Err(SendError::Failed(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn handler_rejects_other_user_and_bad_words() {
        let (a, b) = UnixStream::pair().unwrap();
        assert_eq!(peer_uid(&a), Some(agent_app_core::sys::euid()));
        let called = std::cell::Cell::new(0);
        // Wrong expected uid: nothing is read or run.
        let mut other = b.try_clone().unwrap();
        other.write_all(b"quit\n").unwrap();
        assert_eq!(handle(a.try_clone().unwrap(), agent_app_core::sys::euid().wrapping_add(1), &|_| called.set(called.get() + 1)), Err("peer is not this user"));
        assert_eq!(called.get(), 0);
        // Right uid but not a command word.
        let (c, mut d) = UnixStream::pair().unwrap();
        d.write_all(b"run rm -rf\n").unwrap();
        assert_eq!(handle(c, agent_app_core::sys::euid(), &|_| called.set(called.get() + 1)), Err("unknown command"));
        let mut reply = [0u8; 4];
        d.read_exact(&mut reply).unwrap();
        assert_eq!(&reply, b"err\n");
        // Overlong line.
        let (e, mut f) = UnixStream::pair().unwrap();
        f.write_all(&[b'x'; 200]).unwrap();
        assert_eq!(handle(e, agent_app_core::sys::euid(), &|_| called.set(called.get() + 1)), Err("line too long"));
        assert_eq!(called.get(), 0);
    }

    #[test]
    fn simultaneous_claimers_over_a_stale_socket_get_exactly_one_owner() {
        for round in 0..20 {
            let dir = private_dir(&format!("race{round}"));
            drop(UnixListener::bind(dir.join(SOCKET_NAME)).unwrap());
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let handles: Vec<_> = (0..2).map(|_| {
                let (dir, barrier) = (dir.clone(), barrier.clone());
                std::thread::spawn(move || { barrier.wait(); claim(&dir) })
            }).collect();
            let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap().unwrap()).collect();
            let mine = results.iter().filter(|c| matches!(c, Claim::Mine(_))).count();
            let other = results.iter().filter(|c| matches!(c, Claim::Other)).count();
            assert_eq!((mine, other), (1, 1), "round {round}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn control_is_ok_only_for_our_live_socket() {
        let dir = private_dir("ok");
        let Claim::Mine(listener) = claim(&dir).unwrap() else { panic!("expected to own the socket") };
        let path = dir.join(SOCKET_NAME);
        let inode = std::fs::symlink_metadata(&path).unwrap().ino();
        let alive = serve(listener, |_| {});
        assert!(control_ok(&path, inode, &alive));
        assert!(!control_ok(&path, inode + 1, &alive), "a different inode is not our socket");
        alive.store(false, Ordering::Release);
        assert!(!control_ok(&path, inode, &alive), "a stopped serve loop must not allow hiding");
        alive.store(true, Ordering::Release);
        std::fs::remove_file(&path).unwrap();
        assert!(!control_ok(&path, inode, &alive), "a removed socket must not allow hiding");
        std::fs::write(&path, b"x").unwrap();
        assert!(!control_ok(&path, inode, &alive), "a regular file is not our socket");
        std::fs::remove_file(&path).unwrap();
        let _other = UnixListener::bind(&path).unwrap();
        assert!(!control_ok(&path, inode, &alive), "somebody else's socket on the path is not ours");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn serve_loop_ends_and_clears_its_flag_after_repeated_accept_errors() {
        let dir = private_dir("accepterr");
        let listener = UnixListener::bind(dir.join(SOCKET_NAME)).unwrap();
        listener.set_nonblocking(true).unwrap(); // accept() now fails with WouldBlock every time
        let alive = serve(listener, |_| {});
        let deadline = Instant::now() + Duration::from_secs(5);
        while alive.load(Ordering::Acquire) && Instant::now() < deadline { std::thread::sleep(Duration::from_millis(20)); }
        assert!(!alive.load(Ordering::Acquire));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_idle_connection_does_not_block_others_and_extras_are_dropped() {
        let dir = private_dir("conc");
        let Claim::Mine(listener) = claim(&dir).unwrap() else { panic!("expected to own the socket") };
        let seen = Arc::new(AtomicUsize::new(0));
        let counter = seen.clone();
        serve(listener, move |_| { counter.fetch_add(1, Ordering::SeqCst); });
        let path = dir.join(SOCKET_NAME);
        let idle: Vec<_> = (0..MAX_CONNECTIONS).map(|_| UnixStream::connect(&path).unwrap()).collect();
        std::thread::sleep(Duration::from_millis(200));
        // All slots are taken by silent peers: the next one is dropped, not queued behind them.
        let started = Instant::now();
        assert!(matches!(send(&dir, Command::Show), Err(SendError::Failed(_))));
        assert!(started.elapsed() < Duration::from_millis(1500), "must not wait for the idle connections");
        assert_eq!(seen.load(Ordering::SeqCst), 0);
        drop(idle);
        std::thread::sleep(Duration::from_millis(300));
        // One silent peer leaves room for another connection, which is answered at once.
        let _one = UnixStream::connect(&path).unwrap();
        let started = Instant::now();
        send(&dir, Command::Show).unwrap();
        assert!(started.elapsed() < Duration::from_millis(1500));
        assert_eq!(seen.load(Ordering::SeqCst), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_in_use_sees_a_held_sessions_lease() {
        let dir = private_dir("store");
        assert!(!store_in_use(&dir), "no sessions folder, nothing held");
        std::fs::DirBuilder::new().mode(0o700).create(dir.join("sessions")).unwrap();
        let lock = dir.join("sessions").join("manager.lock");
        let holder = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&lock).unwrap();
        assert!(!store_in_use(&dir), "a lock file nobody holds");
        assert_eq!(unsafe { libc::flock(holder.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
        assert!(store_in_use(&dir));
        drop(holder);
        assert!(!store_in_use(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn terminating_signals_arrive_through_the_pipe() {
        let fd = install_signal_pipe().unwrap();
        unsafe { libc::raise(libc::SIGHUP); }
        let mut byte = [0u8; 1];
        assert_eq!(unsafe { libc::read(fd, byte.as_mut_ptr() as *mut libc::c_void, 1) }, 1);
        assert_eq!(byte[0] as i32, libc::SIGHUP);
        restore_signal_defaults();
    }
}
