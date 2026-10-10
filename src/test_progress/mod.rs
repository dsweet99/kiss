use std::sync::Mutex;

mod heartbeat;
pub use heartbeat::ProgressWatchdog;

static PROGRESS_LOCK: Mutex<()> = Mutex::new(());

pub fn emit_progress(message: &str) {
    heartbeat::note_progress();
    heartbeat::note_work_status(message);
    crate::watch_report::record_watch_report_line(message);
    let _guard = PROGRESS_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    #[cfg(unix)]
    {
        let mut line = Vec::with_capacity(message.len() + 1);
        line.extend_from_slice(message.as_bytes());
        line.push(b'\n');
        unsafe {
            let _ = libc::write(libc::STDOUT_FILENO, line.as_ptr().cast(), line.len());
        }
    }
    #[cfg(not(unix))]
    {
        use std::io::Write;
        println!("{message}");
        let _ = std::io::stdout().flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn work_type_line_updates_stored_status() {
        let _guard = heartbeat::work_status_test_guard();
        heartbeat::set_work_status("working");
        emit_progress("kiss test: Running nextest");
        assert_eq!(heartbeat::current_work_status(), "Running nextest");
        emit_progress("PASS: tests/a.py::test_a (0.01s)");
        assert_eq!(heartbeat::current_work_status(), "Running nextest");
    }

    #[test]
    fn any_printed_line_resets_stage_heartbeat() {
        heartbeat::note_progress();
        std::thread::sleep(Duration::from_millis(5));
        let before = heartbeat::last_emit_age();
        emit_progress("PASS: tests/a.py::test_a (0.01s)");
        let after_pass = heartbeat::last_emit_age();
        assert!(
            after_pass < before,
            "PASS: must refresh the stage watchdog: before={before:?} after={after_pass:?}"
        );
        emit_progress("TIMEOUT: tests/c.py::test_c (1.00s)");
        assert!(heartbeat::last_emit_age() < Duration::from_millis(5));
    }
}
