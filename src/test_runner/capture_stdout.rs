//! Shared stdout capture for unit tests that assert printed recap/progress lines.

use std::io::{Read, Write};
use std::sync::Mutex;

#[cfg(unix)]
pub(crate) fn capture_stdout(f: impl FnOnce()) -> String {
    use std::os::fd::FromRawFd;

    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let mut fds = [0; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    let read_fd = fds[0];
    let write_fd = fds[1];
    let old_stdout = unsafe { libc::dup(libc::STDOUT_FILENO) };
    assert!(old_stdout >= 0);
    assert_eq!(
        unsafe { libc::dup2(write_fd, libc::STDOUT_FILENO) },
        libc::STDOUT_FILENO
    );
    unsafe {
        libc::close(write_fd);
    }
    f();
    let _ = std::io::stdout().flush();
    assert_eq!(
        unsafe { libc::dup2(old_stdout, libc::STDOUT_FILENO) },
        libc::STDOUT_FILENO
    );
    unsafe {
        libc::close(old_stdout);
    }
    let mut reader = unsafe { std::fs::File::from_raw_fd(read_fd) };
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).unwrap();
    drop(reader);
    String::from_utf8_lossy(&buf).into_owned()
}
