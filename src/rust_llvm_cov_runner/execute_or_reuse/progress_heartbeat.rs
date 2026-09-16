use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const STAGE_HEARTBEAT: Duration = Duration::from_millis(500);
const WATCHDOG_POLL: Duration = Duration::from_millis(20);
const DEFAULT_WORK_KIND: &str = "working";

struct StatusClocks {
    process_started: Instant,
    status_changed: Instant,
}

fn last_emit() -> &'static Mutex<Option<Instant>> {
    static LAST_EMIT: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    LAST_EMIT.get_or_init(|| Mutex::new(None))
}

fn work_status() -> &'static Mutex<String> {
    static WORK_STATUS: OnceLock<Mutex<String>> = OnceLock::new();
    WORK_STATUS.get_or_init(|| Mutex::new(DEFAULT_WORK_KIND.to_string()))
}

fn status_clocks() -> &'static Mutex<StatusClocks> {
    static CLOCKS: OnceLock<Mutex<StatusClocks>> = OnceLock::new();
    CLOCKS.get_or_init(|| {
        let now = Instant::now();
        Mutex::new(StatusClocks {
            process_started: now,
            status_changed: now,
        })
    })
}

fn lock_work_status() -> std::sync::MutexGuard<'static, String> {
    work_status()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn lock_status_clocks() -> std::sync::MutexGuard<'static, StatusClocks> {
    status_clocks()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn note_progress() {
    *last_emit()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Instant::now());
}

fn reset_run_clocks() {
    let now = Instant::now();
    let mut clocks = lock_status_clocks();
    clocks.process_started = now;
    clocks.status_changed = now;
}

pub(crate) fn set_work_status(status: &str) {
    let mut current = lock_work_status();
    if current.as_str() != status {
        *current = status.to_string();
        drop(current);
        lock_status_clocks().status_changed = Instant::now();
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

fn format_generic_status() -> String {
    let (start_s, change_s) = {
        let clocks = lock_status_clocks();
        (
            clocks.process_started.elapsed().as_secs(),
            clocks.status_changed.elapsed().as_secs(),
        )
    };
    format!(
        "kiss test: {start_s}s {change_s}s {}",
        current_work_status()
    )
}

fn work_status_from_message(message: &str) -> Option<&str> {
    if let Some(kind) = message
        .strip_prefix("kiss test: ")
        .filter(|kind| kind.starts_with("Running "))
    {
        return Some(kind);
    }
    const KINDS: &[(&str, &str)] = &[
        ("kiss test: Planning", "Planning"),
        ("kiss test: Waiting", "Waiting"),
        ("kiss test: Starting", "Starting"),
        ("kiss test: refreshing", "refreshing coverage"),
        ("kiss test: waiting for", "waiting for coverage refresh"),
        ("kiss test: rslip", "rslip"),
        ("kiss test: tests_remaining", "tests_remaining"),
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
        reset_run_clocks();
        set_work_status(DEFAULT_WORK_KIND);
        note_progress();
        let stop = Arc::new(AtomicBool::new(false));
        let tick_stop = Arc::clone(&stop);
        std::thread::spawn(move || loop {
            std::thread::sleep(WATCHDOG_POLL);
            if tick_stop.load(Ordering::Relaxed) {
                break;
            }
            if silence_exceeds_heartbeat() {
                let status = format_generic_status();
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
fn age_clocks(start: Duration, change: Duration) {
    let now = Instant::now();
    let mut clocks = lock_status_clocks();
    clocks.process_started = now - start;
    clocks.status_changed = now - change;
}

#[cfg(test)]
fn age_process_clock(start: Duration) {
    lock_status_clocks().process_started = Instant::now() - start;
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
        super::set_work_status(super::DEFAULT_WORK_KIND);
        super::note_work_status("PASS: tests/a.py::test_a (0.01s)");
        super::note_work_status("kiss test: Ran cargo 1.0ms");
        assert_eq!(super::current_work_status(), super::DEFAULT_WORK_KIND);
        super::note_work_status("kiss test: tests_remaining=3");
        assert_eq!(super::current_work_status(), "tests_remaining");
        super::note_work_status("kiss test: Running cargo");
        assert_eq!(super::current_work_status(), "Running cargo");
        super::note_work_status("FAIL: tests/b.py::test_b (0.01s)");
        assert_eq!(super::current_work_status(), "Running cargo");
        super::note_work_status("kiss test: Planning ...");
        assert_eq!(super::current_work_status(), "Planning");
        super::note_work_status("kiss test: rslip prepared hits=0 misses=1");
        assert_eq!(super::current_work_status(), "rslip");
        super::note_work_status("kiss test: Waiting");
        assert_eq!(super::current_work_status(), "Waiting");
    }

    #[test]
    fn generic_status_includes_start_and_change_clocks() {
        let _guard = super::work_status_test_guard();
        super::set_work_status(super::DEFAULT_WORK_KIND);
        super::age_clocks(Duration::from_secs(123), Duration::from_secs(45));
        assert_eq!(
            super::format_generic_status(),
            "kiss test: 123s 45s working"
        );
        super::set_work_status("tests_remaining");
        super::age_process_clock(Duration::from_secs(123));
        assert_eq!(
            super::format_generic_status(),
            "kiss test: 123s 0s tests_remaining"
        );
    }

    #[test]
    fn watchdog_prints_generic_status_with_clocks() {
        let src = include_str!("progress_heartbeat.rs");
        assert!(
            src.contains("let status = format_generic_status();"),
            "watchdog must format start and status-change clocks"
        );
        assert!(
            src.contains("emit_progress(&status)"),
            "watchdog must print the formatted generic status"
        );
        let emit = include_str!("progress.rs");
        assert!(
            emit.contains("note_work_status(message)"),
            "emit_progress must store work type independently of printing"
        );
    }
}
