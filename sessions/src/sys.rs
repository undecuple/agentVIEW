//! Linux-only PTY/process operations. No command strings or shell evaluation.
#![allow(unsafe_code)]
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

pub fn uid() -> u32 { unsafe { libc::geteuid() } }

pub fn lock(file: &File) -> io::Result<()> {
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn nonblocking(fd: &impl AsRawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Open an exact relative usage file beneath a trusted provider root, refusing
/// every symlink, magic link and escape. No fallback on unsupported kernels.
pub fn usage_file(root: &std::path::Path, relative: &std::path::Path) -> io::Result<File> {
    use std::os::unix::ffi::OsStrExt;
    let root = std::ffi::CString::new(root.as_os_str().as_bytes())?;
    let relative = std::ffi::CString::new(relative.as_os_str().as_bytes())?;
    let fd = unsafe { libc::open(root.as_ptr(),libc::O_PATH|libc::O_DIRECTORY|libc::O_NOFOLLOW|libc::O_CLOEXEC) };
    if fd < 0 { return Err(io::Error::last_os_error()); }
    let directory = unsafe { File::from_raw_fd(fd) };
    #[repr(C)] struct How { flags: u64, mode: u64, resolve: u64 }
    let how = How { flags:(libc::O_RDONLY|libc::O_NONBLOCK|libc::O_CLOEXEC) as u64,mode:0,resolve:0x02|0x04|0x08 };
    let fd = unsafe { libc::syscall(libc::SYS_openat2,directory.as_raw_fd(),relative.as_ptr(),&how,std::mem::size_of::<How>()) };
    if fd < 0 { return Err(io::Error::last_os_error()); }
    Ok(unsafe { File::from_raw_fd(fd as i32) })
}

/// Stdio protocol children have their own group and die with their launching
/// thread. The synchronous bootstrap retains that thread until child cleanup.
pub fn isolate(command: &mut Command) {
    let parent = unsafe { libc::getpid() };
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() < 0 || libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) < 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::getppid() != parent { return Err(io::Error::from_raw_os_error(libc::ECHILD)); }
            Ok(())
        });
    }
}

pub fn pty(command: &mut Command) -> io::Result<File> {
    let (mut master, mut slave) = (-1, -1);
    let size = libc::winsize { ws_row: 24, ws_col: 100, ws_xpixel: 0, ws_ypixel: 0 };
    if unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null(), &size) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let master = unsafe { File::from_raw_fd(master) };
    let slave = unsafe { File::from_raw_fd(slave) };
    // openpty has no atomic CLOEXEC variant. All app-owned copies are closed on
    // exec; the slave copies are explicitly dup2'd by std::process for stdio.
    for fd in [master.as_raw_fd(), slave.as_raw_fd()] {
        if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    command.stdin(Stdio::from(slave.try_clone()?));
    command.stdout(Stdio::from(slave.try_clone()?));
    command.stderr(Stdio::from(slave));
    let parent = unsafe { libc::getpid() };
    // This closure performs only async-signal-safe syscalls between fork/exec.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) < 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::getppid() != parent {
                return Err(io::Error::from_raw_os_error(libc::ECHILD));
            }
            Ok(())
        });
    }
    Ok(master)
}

pub fn resize(master: &File, cols: u16, rows: u16) -> io::Result<()> {
    let size = libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
    if unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ as _, &size) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Observe exit without reaping: retain the PID until its process group has
/// been signalled, so cleanup cannot target a newly reused process-group ID.
pub fn exited(pid: u32) -> io::Result<bool> {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    if unsafe { libc::waitid(libc::P_PID, pid, &mut info, libc::WEXITED | libc::WNOHANG | libc::WNOWAIT) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { info.si_pid() } != 0)
}

pub fn signal_group(pid: u32, signal: i32) {
    // Only call while this manager still owns the unreaped child PID.
    unsafe { libc::kill(-(pid as i32), signal); }
}
