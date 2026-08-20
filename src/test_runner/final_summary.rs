use std::io::{IsTerminal, Write};
use std::time::Duration;

use super::duration::format_test_duration;
use super::runners::SelectorExecutionSummary;

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
    let mut line = format!("{icon} {} passed", summary.passed);
    if summary.failed > 0 {
        line.push_str(&format!(" · {} failed", summary.failed));
    }
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
    let text = format_final_test_summary(summary, total_duration, stdout_color_enabled());
    println!("{text}");
    let _ = std::io::stdout().flush();
}

#[cfg(test)]
#[path = "final_summary_test.rs"]
mod tests;
