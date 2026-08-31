use std::cell::RefCell;
use std::collections::HashSet;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

type LiveRustHook = Box<dyn FnMut(&str, &str, f64)>;

thread_local! {
    static LIVE_HOOK: RefCell<Option<LiveRustHook>> = const { RefCell::new(None) };
    static LIVE_PRINTED: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    static LIVE_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub fn install_live_rust_test_hook(hook: impl FnMut(&str, &str, f64) + 'static) {
    LIVE_PRINTED.with(|printed| printed.borrow_mut().clear());
    LIVE_ERROR.with(|err| *err.borrow_mut() = None);
    LIVE_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

pub fn clear_live_rust_test_hook() {
    LIVE_HOOK.with(|slot| *slot.borrow_mut() = None);
}

pub fn set_live_rust_error(message: String) {
    LIVE_ERROR.with(|err| {
        if err.borrow().is_none() {
            *err.borrow_mut() = Some(message);
        }
    });
}

#[must_use]
pub fn take_live_rust_error() -> Option<String> {
    LIVE_ERROR.with(|err| err.borrow_mut().take())
}

pub fn mark_live_rust_printed(id: &str) {
    LIVE_PRINTED.with(|printed| {
        printed.borrow_mut().insert(id.to_string());
    });
}

#[must_use]
pub fn live_rust_was_printed(id: &str) -> bool {
    LIVE_PRINTED.with(|printed| printed.borrow().contains(id))
}

static PROGRESS_LOCK: Mutex<()> = Mutex::new(());

pub fn emit_progress(message: &str) {
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
        println!("{message}");
        let _ = std::io::stdout().flush();
    }
}

pub(crate) fn format_ran(name: &str, duration: Duration) -> String {
    format!(
        "kiss test: Ran {name} {:.1}ms",
        duration.as_secs_f64() * 1000.0
    )
}

pub(crate) fn running_line(name: &str) -> String {
    format!("kiss test: Running {name}")
}

pub(crate) fn log_named_step<T, E>(
    name: &str,
    step: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    emit_progress(&running_line(name));
    let started = Instant::now();
    let result = step();
    emit_progress(&format_ran(name, started.elapsed()));
    result
}

enum CargoNextestPhase {
    Cargo,
    Nextest,
}

pub(crate) struct CargoNextestProgress {
    sink: ProgressSink,
    phase: CargoNextestPhase,
    cargo_started: Instant,
    nextest_started: Option<Instant>,
    finished: bool,
}

enum ProgressSink {
    Stdout,
    #[cfg(test)]
    Capture(Arc<Mutex<Vec<String>>>),
}

pub(crate) struct FinishCargoNextestProgress {
    pub progress: Arc<Mutex<CargoNextestProgress>>,
    pub stop: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for FinishCargoNextestProgress {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        self.progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .finish();
    }
}

impl CargoNextestProgress {
    pub(crate) fn start() -> Self {
        Self::start_with_sink(ProgressSink::Stdout)
    }

    fn start_with_sink(sink: ProgressSink) -> Self {
        let progress = Self {
            sink,
            phase: CargoNextestPhase::Cargo,
            cargo_started: Instant::now(),
            nextest_started: None,
            finished: false,
        };
        progress.emit(&running_line("cargo"));
        progress
    }

    pub(crate) fn observe_line(&mut self, line: &[u8]) {
        if self.finished {
            return;
        }
        if matches!(self.phase, CargoNextestPhase::Nextest) {
            emit_live_libtest_event(line);
            return;
        }
        if !line_starts_nextest(line) {
            return;
        }
        self.emit(&format_ran("cargo", self.cargo_started.elapsed()));
        self.emit(&running_line("nextest"));
        self.phase = CargoNextestPhase::Nextest;
        self.nextest_started = Some(Instant::now());
        emit_live_libtest_event(line);
    }

    pub(crate) fn tick(&mut self, elapsed: Duration) {
        if self.finished || !matches!(self.phase, CargoNextestPhase::Cargo) {
            return;
        }
        let secs = elapsed.as_secs();
        if secs > 0 && secs.is_multiple_of(2) {
            self.emit(&format!("kiss test: cargo {secs}s"));
        }
    }

    pub(crate) fn finish(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        match self.phase {
            CargoNextestPhase::Cargo => {
                self.emit(&format_ran("cargo", self.cargo_started.elapsed()));
            }
            CargoNextestPhase::Nextest => {
                let started = self.nextest_started.unwrap_or(self.cargo_started);
                self.emit(&format_ran("nextest", started.elapsed()));
            }
        }
    }

    fn emit(&self, message: &str) {
        match &self.sink {
            ProgressSink::Stdout => emit_progress(message),
            #[cfg(test)]
            ProgressSink::Capture(lines) => lines
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(message.to_string()),
        }
    }
}

fn line_starts_nextest(line: &[u8]) -> bool {
    let line = trim_ascii_line(line);
    if line.is_empty() {
        return false;
    }
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return false;
    };
    if value.get("type").is_some() {
        return true;
    }
    value.get("reason").and_then(Value::as_str) == Some("build-finished")
}

fn emit_live_libtest_event(line: &[u8]) {
    LIVE_HOOK.with(|slot| {
        let mut hook_slot = slot.borrow_mut();
        let Some(hook) = hook_slot.as_mut() else {
            return;
        };
        let Ok(value) = serde_json::from_slice::<Value>(trim_ascii_line(line)) else {
            return;
        };
        if value.get("type").and_then(Value::as_str) != Some("test") {
            return;
        }
        let event = value.get("event").and_then(Value::as_str).unwrap_or("");
        if !matches!(event, "ok" | "failed" | "timeout" | "timed_out") {
            return;
        }
        let Some(name) = value.get("name").and_then(Value::as_str) else {
            return;
        };
        let exec_time = value
            .get("exec_time")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        hook(name, event, exec_time);
    });
}

fn trim_ascii_line(line: &[u8]) -> &[u8] {
    let mut line = line;
    while line.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
        line = &line[..line.len() - 1];
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replay(stdout: &[u8]) -> Vec<String> {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let mut progress =
            CargoNextestProgress::start_with_sink(ProgressSink::Capture(Arc::clone(&captured)));
        for line in stdout.split(|b| *b == b'\n') {
            progress.observe_line(line);
        }
        progress.finish();
        captured
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn names(lines: &[String]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.split_whitespace()
                    .take(4)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }

    #[test]
    fn cargo_heartbeat_ticks_only_during_cargo_phase() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let mut progress =
            CargoNextestProgress::start_with_sink(ProgressSink::Capture(Arc::clone(&captured)));
        progress.tick(Duration::from_secs(2));
        progress.tick(Duration::from_secs(4));
        progress.observe_line(br#"{"reason":"build-finished","success":true}"#);
        progress.tick(Duration::from_secs(6));
        progress.finish();
        let lines = captured
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert!(
            lines.iter().any(|line| line == "kiss test: cargo 2s"),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|line| line == "kiss test: cargo 4s"),
            "{lines:?}"
        );
        assert!(
            !lines.iter().any(|line| line.contains("cargo 6s")),
            "no heartbeats after nextest: {lines:?}"
        );
    }

    #[test]
    fn cargo_then_nextest_when_build_finished_then_libtest() {
        let stdout = br#"{"reason":"compiler-artifact","executable":"/tmp/bin"}
{"reason":"build-finished","success":true}
{"type":"test","event":"started","name":"pkg::bin$case"}
"#;
        let lines = replay(stdout);
        assert_eq!(
            names(&lines),
            [
                "kiss test: Running cargo",
                "kiss test: Ran cargo",
                "kiss test: Running nextest",
                "kiss test: Ran nextest",
            ]
        );
        assert!(lines[1].contains("ms"), "{}", lines[1]);
        assert!(lines[3].contains("ms"), "{}", lines[3]);
    }

    #[test]
    fn cargo_only_when_build_never_finishes() {
        let stdout = br#"{"reason":"compiler-artifact","executable":"/tmp/bin"}
"#;
        let lines = replay(stdout);
        assert_eq!(
            names(&lines),
            ["kiss test: Running cargo", "kiss test: Ran cargo",]
        );
    }

    #[test]
    fn live_hook_emits_terminal_libtest_events() {
        let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
        let captured = std::sync::Arc::clone(&seen);
        install_live_rust_test_hook(move |name, event, exec_time| {
            captured
                .lock()
                .unwrap()
                .push((name.to_string(), event.to_string(), exec_time));
        });
        let _ = replay(
            br#"{"reason":"build-finished","success":true}
{"type":"test","event":"ok","name":"pkg::bin$case","exec_time":0.25}
"#,
        );
        clear_live_rust_test_hook();
        let events = seen.lock().unwrap().clone();
        assert_eq!(events, vec![("pkg::bin$case".into(), "ok".into(), 0.25)]);
    }

    #[test]
    fn libtest_without_build_finished_still_switches_to_nextest() {
        let stdout = br#"{"type":"test","event":"started","name":"pkg::bin$case"}
"#;
        let lines = replay(stdout);
        assert_eq!(
            names(&lines),
            [
                "kiss test: Running cargo",
                "kiss test: Ran cargo",
                "kiss test: Running nextest",
                "kiss test: Ran nextest",
            ]
        );
    }

    #[test]
    fn named_step_lines_use_one_decimal_ms() {
        assert_eq!(
            format_ran("llvm-cov", Duration::from_millis(1500)),
            "kiss test: Ran llvm-cov 1500.0ms"
        );
        assert_eq!(running_line("llvm-cov"), "kiss test: Running llvm-cov");
    }

    #[test]
    fn cargo_nextest_progress_is_wired_into_batch_subprocess() {
        let src = include_str!("batch_run_subprocess.rs");
        assert!(
            src.contains("CargoNextestProgress::start"),
            "run_tracked_batch_command must start cargo/nextest progress"
        );
        assert!(
            src.contains("read_stdout_tracking_progress"),
            "batch stdout must be observed for cargo→nextest transition"
        );
    }

    #[test]
    fn llvm_cov_progress_is_wired_into_fresh_export() {
        let src = include_str!("batch_executor_fresh.rs");
        assert!(
            src.contains("\"llvm-cov\""),
            "fresh export must log Running/Ran llvm-cov"
        );
        assert!(
            src.matches("\"llvm-cov\"").count() >= 2,
            "selector-entry and check-aggregate exports must both log llvm-cov"
        );
        let finish = include_str!("batch_executor_finish_check_aggregate.rs");
        assert!(
            finish.contains("\"entry-store\""),
            "check-aggregate finish must log Running/Ran entry-store"
        );
        assert!(
            finish.contains("\"derived-publish\""),
            "check-aggregate finish must log Running/Ran derived-publish"
        );
    }
}
