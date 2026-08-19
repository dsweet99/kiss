
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

pub(crate) fn emit_progress(message: &str) {
    println!("{message}");
    let _ = std::io::stdout().flush();
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

pub(crate) struct FinishCargoNextestProgress(pub Arc<Mutex<CargoNextestProgress>>);

impl Drop for FinishCargoNextestProgress {
    fn drop(&mut self) {
        self.0
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
        if self.finished || matches!(self.phase, CargoNextestPhase::Nextest) {
            return;
        }
        if !line_starts_nextest(line) {
            return;
        }
        self.emit(&format_ran("cargo", self.cargo_started.elapsed()));
        self.emit(&running_line("nextest"));
        self.phase = CargoNextestPhase::Nextest;
        self.nextest_started = Some(Instant::now());
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
            src.contains("log_named_step(\"llvm-cov\""),
            "fresh export must log Running/Ran llvm-cov"
        );
        assert!(
            src.matches("log_named_step(\"llvm-cov\"").count() >= 2,
            "selector-entry and check-aggregate exports must both log llvm-cov"
        );
        let finish = include_str!("batch_executor_finish_check_aggregate.rs");
        assert!(
            finish.contains("log_named_step(\"entry-store\""),
            "check-aggregate finish must log Running/Ran entry-store"
        );
        assert!(
            finish.contains("log_named_step(\"derived-publish\""),
            "check-aggregate finish must log Running/Ran derived-publish"
        );
    }
}
