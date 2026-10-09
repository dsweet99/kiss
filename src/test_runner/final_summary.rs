#![cfg_attr(not(test), allow(dead_code))]
use std::cell::{Cell, RefCell};
use std::io::IsTerminal;
use std::time::Duration;

use kiss::watch_report::WatchSuiteTotals;

use super::duration::format_test_duration;
use super::runners::SelectorExecutionSummary;

thread_local! {
    static DEFER_RECAP: Cell<bool> = const { Cell::new(false) };
    static PENDING_RECAP: RefCell<Option<(FinalTestSummary, Duration)>> = const { RefCell::new(None) };
}

pub(crate) struct RecapDeferGuard {
    discard: Cell<bool>,
}

impl RecapDeferGuard {
    pub(crate) fn enter() -> Self {
        DEFER_RECAP.set(true);
        PENDING_RECAP.with(|slot| *slot.borrow_mut() = None);
        Self {
            discard: Cell::new(false),
        }
    }

    pub(crate) fn discard(&self) {
        self.discard.set(true);
    }
}

impl Drop for RecapDeferGuard {
    fn drop(&mut self) {
        if self.discard.get() {
            DEFER_RECAP.set(false);
            PENDING_RECAP.with(|slot| *slot.borrow_mut() = None);
            return;
        }
        flush_final_test_summary();
    }
}

fn watch_suite_totals(summary: &FinalTestSummary, total_duration: Duration) -> WatchSuiteTotals {
    let timed_out = summary.timed_out_selectors.len();
    WatchSuiteTotals {
        passed: summary.passed,
        failed: summary.failed.saturating_sub(timed_out),
        timed_out,
        total_label: format_test_duration(total_duration),
        max_pass_label: format_max_pass_duration(summary.max_passing_run_duration),
    }
}

fn record_watch_suite_totals(summary: &FinalTestSummary, total_duration: Duration) {
    kiss::watch_report::record_watch_suite_totals(watch_suite_totals(summary, total_duration));
}

fn flush_final_test_summary() {
    DEFER_RECAP.set(false);
    let Some((summary, total_duration)) = PENDING_RECAP.with(|slot| slot.borrow_mut().take())
    else {
        return;
    };
    let text = format_final_test_summary(&summary, total_duration, stdout_color_enabled());
    crate::test_runner::emit_test_progress(&text);
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FinalTestSummary {
    pub(crate) passed: usize,
    pub(crate) failed: usize,
    pub(crate) failed_selectors: Vec<String>,
    pub(crate) timed_out_selectors: Vec<String>,
    pub(crate) max_passing_run_duration: Duration,
}

impl FinalTestSummary {
    pub(crate) fn absorb(summaries: &[&SelectorExecutionSummary]) -> Self {
        let mut total = 0;
        let mut failed = 0;
        let mut failed_selectors = Vec::new();
        let mut timed_out_selectors = Vec::new();
        let mut max_passing_run_duration = Duration::ZERO;
        for summary in summaries {
            total += summary.total;
            failed += summary.failed;
            failed_selectors.extend(summary.failed_selectors.iter().cloned());
            timed_out_selectors.extend(summary.timed_out_selectors.iter().cloned());
            max_passing_run_duration =
                max_passing_run_duration.max(summary.max_passing_run_duration);
        }
        Self {
            passed: total.saturating_sub(failed),
            failed,
            failed_selectors,
            timed_out_selectors,
            max_passing_run_duration,
        }
    }
}

pub(crate) fn format_max_pass_duration(duration: Duration) -> String {
    if duration.is_zero() {
        "0s".to_string()
    } else {
        format_test_duration(duration)
    }
}

pub(crate) fn format_final_test_summary(
    summary: &FinalTestSummary,
    total_duration: Duration,
    color: bool,
) -> String {
    let icon = if summary.failed == 0 { "✓" } else { "✗" };
    let icon = if color {
        if summary.failed == 0 {
            format!("\x1b[32m{icon}\x1b[0m")
        } else {
            format!("\x1b[31m{icon}\x1b[0m")
        }
    } else {
        icon.to_string()
    };
    let timed_out = summary.timed_out_selectors.len();
    let failed = summary.failed.saturating_sub(timed_out);
    let mut line = format!(
        "{icon} {} passed · {} failed · {} timed out",
        summary.passed, failed, timed_out
    );
    line.push_str(&format!(
        " · {} total · {} max pass",
        format_test_duration(total_duration),
        format_max_pass_duration(summary.max_passing_run_duration)
    ));
    let mut lines = vec![line];
    for selector in &summary.failed_selectors {
        let failed = if color { "\x1b[31mFAIL\x1b[0m" } else { "FAIL" };
        lines.push(format!("{failed} {selector}"));
    }
    for selector in &summary.timed_out_selectors {
        let timed_out = if color {
            "\x1b[31mTIMEOUT\x1b[0m"
        } else {
            "TIMEOUT"
        };
        lines.push(format!("{timed_out} {selector}"));
    }
    lines.join("\n")
}

pub(crate) fn stdout_color_enabled() -> bool {
    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none()
}

pub(crate) fn print_final_test_summary(summary: &FinalTestSummary, total_duration: Duration) {
    record_watch_suite_totals(summary, total_duration);
    if DEFER_RECAP.get() {
        PENDING_RECAP.with(|slot| {
            *slot.borrow_mut() = Some((summary.clone(), total_duration));
        });
        return;
    }
    let text = format_final_test_summary(summary, total_duration, stdout_color_enabled());
    crate::test_runner::emit_test_progress(&text);
}

#[cfg(test)]
#[path = "final_summary_test.rs"]
mod tests;
