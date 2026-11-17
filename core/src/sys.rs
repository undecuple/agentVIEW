//! The only module with unsafe code: openat2 and geteuid. Audit this file first.
#![allow(unsafe_code)]
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

#[repr(C)]
struct OpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}
const RESOLVE_NO_MAGICLINKS: u64 = 0x02;
const RESOLVE_NO_SYMLINKS: u64 = 0x04;
const RESOLVE_BENEATH: u64 = 0x08;

fn cstr(p: &Path) -> io::Result<std::ffi::CString> {
    std::ffi::CString::new(p.as_os_str().as_bytes()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

/// Open a directory as a path-only handle, not following a final symlink.
fn open_dir(p: &Path) -> io::Result<OwnedFd> {
    let c = cstr(p)?;
    let fd = unsafe { libc::open(c.as_ptr(), libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// Open `rel` below `root` for reading, refusing any symlink, magic link or escape from `root`.
/// Never blocks (O_NONBLOCK). Fails if the kernel has no openat2 (Linux 5.6+).
pub fn open_beneath(root: &Path, rel: &Path) -> io::Result<File> {
    let dir = open_dir(root)?;
    let c = cstr(rel)?;
    let how = OpenHow {
        flags: (libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NONBLOCK) as u64,
        mode: 0,
        resolve: RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS,
    };
    let fd = unsafe {
        libc::syscall(libc::SYS_openat2, dir.as_raw_fd(), c.as_ptr(), &how as *const OpenHow, std::mem::size_of::<OpenHow>())
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd as i32) })
}

pub fn euid() -> u32 {
    unsafe { libc::geteuid() }
}
