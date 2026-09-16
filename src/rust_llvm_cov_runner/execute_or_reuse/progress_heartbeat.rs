use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const STAGE_HEARTBEAT: Duration = Duration::from_millis(80);
const WATCHDOG_POLL: Duration = Duration::from_millis(20);

fn last_emit() -> &'static Mutex<Option<Instant>> {
    static LAST_EMIT: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    LAST_EMIT.get_or_init(|| Mutex::new(None))
}

pub fn note_progress() {
    *last_emit()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Instant::now());
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
                super::progress::emit_progress("kiss test: working");
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
mod tests {
    use super::STAGE_HEARTBEAT;
    use std::time::Duration;

    #[test]
    fn stage_heartbeat_is_under_progress_gap_target() {
        assert!(STAGE_HEARTBEAT <= Duration::from_millis(100));
    }
}
