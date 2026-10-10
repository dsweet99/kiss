use std::io::{Read, Write};
use std::sync::Mutex;

#[cfg(unix)]
static CAPTURE_LOCK: Mutex<()> = Mutex::new(());

#[cfg(unix)]
pub(crate) fn capture_stdout(f: impl FnOnce()) -> String {
    capture_one(libc::STDOUT_FILENO, f, || {
        let _ = std::io::stdout().flush();
    })
}

#[cfg(unix)]
fn capture_one(target_fd: i32, f: impl FnOnce(), flush: impl FnOnce()) -> String {
    let _guard = CAPTURE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let pipe = open_pipe();
    let old_fd = dup_fd(target_fd);
    redirect_fd(pipe.write, target_fd);
    close_fd(pipe.write);
    f();
    flush();
    redirect_fd(old_fd, target_fd);
    close_fd(old_fd);
    read_pipe_to_string(pipe.read)
}

#[cfg(unix)]
struct PipeFds {
    read: i32,
    write: i32,
}

#[cfg(unix)]
fn open_pipe() -> PipeFds {
    let mut fds = [0; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
    PipeFds {
        read: fds[0],
        write: fds[1],
    }
}

#[cfg(unix)]
fn dup_fd(fd: i32) -> i32 {
    let duped = unsafe { libc::dup(fd) };
    assert!(duped >= 0);
    duped
}

#[cfg(unix)]
fn redirect_fd(from: i32, to: i32) {
    assert_eq!(unsafe { libc::dup2(from, to) }, to);
}

#[cfg(unix)]
fn close_fd(fd: i32) {
    unsafe {
        libc::close(fd);
    }
}

#[cfg(unix)]
fn read_pipe_to_string(read_fd: i32) -> String {
    use std::os::fd::FromRawFd;

    let mut reader = unsafe { std::fs::File::from_raw_fd(read_fd) };
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).unwrap();
    drop(reader);
    String::from_utf8_lossy(&buf).into_owned()
}
