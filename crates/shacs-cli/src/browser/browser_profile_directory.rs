//! An opened profile identity. Locking it covers the interval before Chromium
//! creates SingletonLock even if all three sibling lock files are replaced.
use super::browser_lock::FileIdentity;
use std::ffi::{CStr, CString};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

pub(super) fn open(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(path)
}

/// Opens a directory relative to a live descriptor, never following its name.
fn child(parent: &File, name: &CStr) -> io::Result<File> {
    // SAFETY: parent stays open and name is a terminated C string. openat
    // returns a new owned descriptor; no pointer escapes this call.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fd was just opened successfully and has no other owner.
    Ok(unsafe { File::from_raw_fd(fd) })
}

pub(super) fn profile(parent: &File) -> io::Result<File> {
    child(parent, c"profile")
}

/// Reads only the SingletonLock symlink of this opened directory. A regular
/// file produces EINVAL and is therefore treated as an unrecognizable lock.
pub(super) fn singleton(directory: &File) -> io::Result<String> {
    let mut bytes = [0u8; 1024];
    // SAFETY: the directory is live, the name is terminated, and the output
    // buffer is writable for exactly the length supplied.
    let count = unsafe {
        libc::readlinkat(
            directory.as_raw_fd(),
            c"SingletonLock".as_ptr(),
            bytes.as_mut_ptr().cast(),
            bytes.len(),
        )
    };
    let count = usize::try_from(count).map_err(|_| io::Error::last_os_error())?;
    if count == bytes.len() {
        return Err(io::Error::other("truncated SingletonLock"));
    }
    String::from_utf8(bytes[..count].to_vec()).map_err(io::Error::other)
}

/// Owns the duplicate descriptor transferred to fdopendir, including on an
/// error while walking. It never crosses a thread boundary.
struct Entries(*mut libc::DIR);

impl Drop for Entries {
    fn drop(&mut self) {
        // SAFETY: only a successful fdopendir constructs Entries; this is
        // its sole owner and no readdir borrow survives into Drop.
        unsafe { libc::closedir(self.0) };
    }
}

fn names(directory: &File) -> io::Result<Vec<CString>> {
    let fd = directory.try_clone()?.into_raw_fd();
    // SAFETY: fd is a new owned descriptor of a directory. On success
    // fdopendir takes ownership; on failure it leaves fd with this function.
    let raw = unsafe { libc::fdopendir(fd) };
    if raw.is_null() {
        let error = io::Error::last_os_error();
        // SAFETY: fdopendir failed, so fd still belongs to this function.
        drop(unsafe { File::from_raw_fd(fd) });
        return Err(error);
    }
    let entries = Entries(raw);
    let mut names = Vec::new();
    loop {
        // SAFETY: errno is thread local, and both supported targets expose
        // its address. Clearing it distinguishes EOF from a readdir error.
        unsafe {
            #[cfg(target_os = "linux")]
            {
                *libc::__errno_location() = 0;
            }
            #[cfg(target_os = "macos")]
            {
                *libc::__error() = 0;
            }
        }
        // SAFETY: entries uniquely owns a live DIR, used by this thread only.
        let entry = unsafe { libc::readdir(entries.0) };
        if entry.is_null() {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(0) {
                Ok(names)
            } else {
                Err(error)
            };
        }
        // SAFETY: readdir returned a live dirent with a terminated d_name.
        // Copy before the next readdir call invalidates that storage.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_owned();
        if name != c"." && name != c".." {
            names.push(name);
        }
    }
}

fn unlink(parent: &File, name: &CStr, flags: i32) -> io::Result<()> {
    // SAFETY: parent remains open, name is terminated; unlinkat retains no
    // pointers. AT_REMOVEDIR never recursively follows a replaced name.
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), flags) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn empty(directory: &File) -> io::Result<()> {
    for name in names(directory)? {
        match child(directory, &name) {
            Ok(nested) => {
                empty(&nested)?;
                unlink(directory, &name, libc::AT_REMOVEDIR)?;
            }
            Err(error) if matches!(error.raw_os_error(), Some(libc::ENOTDIR | libc::ELOOP)) => {
                unlink(directory, &name, 0)?;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Only this opened identity is traversed, even if browser/profile is renamed.
/// The final rmdir is non-recursive and is attempted only at the same identity.
pub(super) fn remove(directory: &File, parent: &File, path: &Path) -> io::Result<()> {
    empty(directory)?;
    let current = profile(parent)?;
    if FileIdentity::of(&current.metadata()?) != FileIdentity::of(&directory.metadata()?) {
        return Err(io::Error::other("profile path replaced during removal"));
    }
    // Also reject replacement of browser/ itself. Contents above were removed
    // through descriptors, never through this potentially changed path.
    if FileIdentity::of(&fs::symlink_metadata(path)?) != FileIdentity::of(&directory.metadata()?) {
        return Err(io::Error::other("browser path replaced during removal"));
    }
    unlink(parent, c"profile", libc::AT_REMOVEDIR)
}
