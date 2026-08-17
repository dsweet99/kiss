//! Capture and filter cycle stdout for one-shot watcher clients.

use std::io::{Read, Write};
use std::sync::Mutex;

fn stdout_redirect_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Capture stdout to a tempfile (no pipe-buffer deadlock), then replay to the
/// original stdout so the watcher tty still sees the cycle.
#[cfg(unix)]
pub(crate) fn tee_stdout<R>(f: impl FnOnce() -> R) -> (R, String) {
    use std::os::fd::AsRawFd;

    let _guard = stdout_redirect_lock();

    let path = {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "kiss-watch-tee-{}-{}.log",
            std::process::id(),
            crate::test_runner::rust_coverage_index::unique_suffix()
        ));
        p
    };
    let write_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(&path)
        .expect("tee_stdout open");
    let write_fd = write_file.as_raw_fd();
    let old_stdout = unsafe { libc::dup(libc::STDOUT_FILENO) };
    assert!(old_stdout >= 0);
    assert_eq!(
        unsafe { libc::dup2(write_fd, libc::STDOUT_FILENO) },
        libc::STDOUT_FILENO
    );
    let result = f();
    let _ = std::io::stdout().flush();
    drop(write_file);
    assert_eq!(
        unsafe { libc::dup2(old_stdout, libc::STDOUT_FILENO) },
        libc::STDOUT_FILENO
    );
    unsafe {
        libc::close(old_stdout);
    }

    let mut buf = Vec::new();
    if let Ok(mut reader) = std::fs::File::open(&path) {
        let _ = reader.read_to_end(&mut buf);
    }
    let _ = std::fs::remove_file(&path);

    let _ = std::io::stdout().write_all(&buf);
    let _ = std::io::stdout().flush();

    (result, String::from_utf8_lossy(&buf).into_owned())
}

/// Lines a one-shot client should reprint so FAIL / VIOLATION detail matches
/// a non-watcher `kiss test` (without progress noise like Planning / PASS:).
pub(crate) fn extract_client_report(captured: &str) -> String {
    let mut lines = Vec::new();
    for line in captured.lines() {
        if is_client_report_line(line) {
            lines.push(strip_ansi(line));
        }
    }
    lines.join("\n")
}

fn is_client_report_line(line: &str) -> bool {
    let plain = strip_ansi(line);
    let t = plain.trim_start();
    if t.contains("VIOLATION:") {
        return true;
    }
    if t.starts_with("FAIL ") || t.starts_with("TIMEOUT ") {
        return true;
    }
    if t.starts_with("Run 'kiss rules'") {
        return true;
    }
    if t.contains("passed") && t.contains("total") && t.contains("max pass") {
        return true;
    }
    if t.starts_with("  ") && t.contains('%') && t.contains("required") {
        return true;
    }
    false
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c2 in chars.by_ref() {
                    if c2.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_keeps_violations_and_fails() {
        let captured = "\
kiss test: Planning ...
PASS: a::b (0.01s)
✓ 1 passed · 0.1s total · 0s max pass
VIOLATION:test_coverage: codebase coverage 75% below 90% threshold
VIOLATION:test_coverage:lib.py:4:<file>: 75% covered. Add test coverage for this code unit.
Run 'kiss rules' for more information about fixing violations.
kiss: 100ms
";
        let report = extract_client_report(captured);
        assert!(report.contains("VIOLATION:test_coverage: codebase"));
        assert!(report.contains("Run 'kiss rules'"));
        assert!(report.contains("✓ 1 passed"));
        assert!(!report.contains("Planning"));
        assert!(!report.contains("PASS: a::b"));
        assert!(!report.contains("kiss: 100ms"));
    }

    #[test]
    fn extract_keeps_fail_recap() {
        let captured = "\
\u{1b}[31m✗\u{1b}[0m 0 passed · 1 failed · 0.2s total · 0s max pass
\u{1b}[31mFAIL\u{1b}[0m tests/test_a.py::test_a
";
        let report = extract_client_report(captured);
        assert!(report.contains("✗ 0 passed · 1 failed"));
        assert!(report.contains("FAIL tests/test_a.py::test_a"));
        assert!(!report.contains('\u{1b}'));
    }
}
