use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const STAGE_HEARTBEAT: Duration = Duration::from_millis(500);
const WATCHDOG_POLL: Duration = Duration::from_millis(20);
const DEFAULT_WORK_STATUS: &str = "kiss test: working";

fn last_emit() -> &'static Mutex<Option<Instant>> {
    static LAST_EMIT: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    LAST_EMIT.get_or_init(|| Mutex::new(None))
}

fn work_status() -> &'static Mutex<String> {
    static WORK_STATUS: OnceLock<Mutex<String>> = OnceLock::new();
    WORK_STATUS.get_or_init(|| Mutex::new(DEFAULT_WORK_STATUS.to_string()))
}

fn lock_work_status() -> std::sync::MutexGuard<'static, String> {
    work_status()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn note_progress() {
    *last_emit()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Instant::now());
}

pub(crate) fn set_work_status(status: &str) {
    let mut current = lock_work_status();
    if current.as_str() != status {
        *current = status.to_string();
    }
}

pub(crate) fn note_work_status(message: &str) {
    if let Some(status) = work_status_from_message(message) {
        set_work_status(status);
    }
}

pub(crate) fn current_work_status() -> String {
    lock_work_status().clone()
}

fn work_status_from_message(message: &str) -> Option<&str> {
    if message.starts_with("kiss test: Running ") {
        return Some(message);
    }
    const KINDS: &[(&str, &str)] = &[
        ("kiss test: Planning", "kiss test: Planning"),
        ("kiss test: Waiting", "kiss test: Waiting"),
        ("kiss test: Starting", "kiss test: Starting"),
        ("kiss test: refreshing", "kiss test: refreshing coverage"),
        (
            "kiss test: waiting for",
            "kiss test: waiting for coverage refresh",
        ),
        ("kiss test: rslip", "kiss test: rslip"),
    ];
    KINDS
        .iter()
        .find(|(prefix, _)| message.starts_with(*prefix))
        .map(|(_, status)| *status)
}

fn silence_exceeds_heartbeat() -> bool {
    last_emit()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_none_or(|last| last.elapsed() >= STAGE_HEARTBEAT)
}

pub struct ProgressWatchdog {
    stop: Arc<AtomicBool>,
}

impl ProgressWatchdog {
    pub fn start() -> Self {
        note_progress();
        let stop = Arc::new(AtomicBool::new(false));
        let tick_stop = Arc::clone(&stop);
        std::thread::spawn(move || loop {
            std::thread::sleep(WATCHDOG_POLL);
            if tick_stop.load(Ordering::Relaxed) {
                break;
            }
            if silence_exceeds_heartbeat() {
                let status = current_work_status();
                super::progress::emit_progress(&status);
            }
        });
        Self { stop }
    }
}

impl Drop for ProgressWatchdog {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
pub(super) fn last_emit_age() -> Duration {
    last_emit()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .map_or(Duration::MAX, |t| t.elapsed())
}

#[cfg(test)]
pub(crate) fn work_status_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static TEST_GUARD: Mutex<()> = Mutex::new(());
    TEST_GUARD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::STAGE_HEARTBEAT;
    use std::time::Duration;

    #[test]
    fn stage_heartbeat_is_500ms() {
        assert_eq!(STAGE_HEARTBEAT, Duration::from_millis(500));
    }

    #[test]
    fn silence_exceeds_heartbeat_only_after_500ms() {
        super::note_progress();
        assert!(!super::silence_exceeds_heartbeat());
        std::thread::sleep(STAGE_HEARTBEAT + Duration::from_millis(30));
        assert!(super::silence_exceeds_heartbeat());
    }

    #[test]
    fn work_status_updates_only_when_work_type_changes() {
        let _guard = super::work_status_test_guard();
        super::set_work_status(super::DEFAULT_WORK_STATUS);
        super::note_work_status("PASS: tests/a.py::test_a (0.01s)");
        super::note_work_status("kiss test: Ran cargo 1.0ms");
        super::note_work_status("kiss test: tests_remaining=3");
        assert_eq!(super::current_work_status(), super::DEFAULT_WORK_STATUS);
        super::note_work_status("kiss test: Running cargo");
        assert_eq!(super::current_work_status(), "kiss test: Running cargo");
        super::note_work_status("FAIL: tests/b.py::test_b (0.01s)");
        assert_eq!(super::current_work_status(), "kiss test: Running cargo");
        super::note_work_status("kiss test: Planning ...");
        assert_eq!(super::current_work_status(), "kiss test: Planning");
        super::note_work_status("kiss test: rslip prepared hits=0 misses=1");
        assert_eq!(super::current_work_status(), "kiss test: rslip");
        super::note_work_status("kiss test: Waiting");
        assert_eq!(super::current_work_status(), "kiss test: Waiting");
    }

    #[test]
    fn watchdog_prints_stored_status_not_hardcoded_working() {
        let src = include_str!("progress_heartbeat.rs");
        assert!(
            src.contains("let status = current_work_status();"),
            "watchdog must read the stored work-status string"
        );
        assert!(
            src.contains("emit_progress(&status)"),
            "watchdog must print the stored work-status string"
        );
        let emit = include_str!("progress.rs");
        assert!(
            emit.contains("note_work_status(message)"),
            "emit_progress must store work type independently of printing"
        );
    }
}
