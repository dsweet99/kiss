use std::borrow::Cow;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

#[path = "progress_watch_capture_lang.rs"]
mod capture_lang;

thread_local! {
    static PROGRESS_LANG: Cell<Option<crate::Language>> = const { Cell::new(None) };
}

pub struct ProgressLanguageGuard;

impl ProgressLanguageGuard {
    pub fn enter(lang: crate::Language) -> Self {
        PROGRESS_LANG.set(Some(lang));
        Self
    }
}

impl Drop for ProgressLanguageGuard {
    fn drop(&mut self) {
        PROGRESS_LANG.set(None);
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WatchSuiteTotals {
    pub passed: usize,
    pub failed: usize,
    pub timed_out: usize,
    pub total_label: String,
    pub max_pass_label: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchNamedOutcome {
    Pass,
    Fail,
    Timeout,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatchNamed {
    pub lang: crate::Language,
    pub selector: String,
    pub outcome: WatchNamedOutcome,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WatchReportTaken {
    pub lines: Vec<String>,
    pub totals: Option<WatchSuiteTotals>,
    pub named: Vec<WatchNamed>,
    pub lang_passed: [usize; 2],
    pub lang_failed: [usize; 2],
    pub lang_timed_out: [usize; 2],
}

#[derive(Default)]
struct WatchReportCapture {
    lines: Vec<String>,
    totals: Option<WatchSuiteTotals>,
    named: BTreeMap<(crate::Language, String), WatchNamedOutcome>,
    lang_passed: [usize; 2],
    lang_failed: [usize; 2],
    lang_timed_out: [usize; 2],
}

fn watch_report_slot() -> &'static Mutex<Option<WatchReportCapture>> {
    static SLOT: OnceLock<Mutex<Option<WatchReportCapture>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn lock_watch_report() -> std::sync::MutexGuard<'static, Option<WatchReportCapture>> {
    watch_report_slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn begin_watch_report_capture() {
    *lock_watch_report() = Some(WatchReportCapture::default());
}

fn progress_lang() -> Option<crate::Language> {
    PROGRESS_LANG.get()
}

pub fn record_watch_suite_totals(totals: WatchSuiteTotals) {
    if let Some(capture) = lock_watch_report().as_mut() {
        capture.totals = Some(totals);
    }
}

#[must_use]
pub fn take_watch_report_taken() -> Option<WatchReportTaken> {
    lock_watch_report()
        .take()
        .map(WatchReportCapture::into_taken)
}

#[must_use]
pub fn take_watch_report_parts() -> Option<(Vec<String>, Option<WatchSuiteTotals>)> {
    take_watch_report_taken().map(|taken| (taken.lines, taken.totals))
}

#[must_use]
pub fn take_watch_report_lines() -> Option<Vec<String>> {
    take_watch_report_parts().map(|(lines, _)| lines)
}

#[must_use]
pub fn take_watch_report_capture() -> Option<String> {
    Some(compact_watch_report(&take_watch_report_lines()?))
}

pub(crate) fn record_watch_report_line(message: &str) {
    let mut slot = lock_watch_report();
    let Some(capture) = slot.as_mut() else {
        return;
    };
    for line in message.split('\n') {
        if !is_watch_report_line(line) {
            continue;
        }
        capture.lines.push(line.to_string());
        capture_lang::apply(capture, line);
        if let Some(tag) = lang_collapsed_tag(line) {
            capture.lines.push(tag);
        }
    }
}

impl WatchReportCapture {
    fn into_taken(self) -> WatchReportTaken {
        WatchReportTaken {
            lines: self.lines,
            totals: self.totals,
            named: self
                .named
                .into_iter()
                .map(|((lang, selector), outcome)| WatchNamed {
                    lang,
                    selector,
                    outcome,
                })
                .collect(),
            lang_passed: self.lang_passed,
            lang_failed: self.lang_failed,
            lang_timed_out: self.lang_timed_out,
        }
    }
}

fn lang_collapsed_tag(message: &str) -> Option<String> {
    let lang = PROGRESS_LANG.get()?;
    let line = strip_ansi(message.trim());
    if line.starts_with("kiss test: lang_collapsed ") {
        return None;
    }
    let (label, rest) = if let Some(rest) = line.strip_prefix("PASS") {
        ("pass", rest)
    } else if let Some(rest) = line.strip_prefix("TIMEOUT") {
        ("timeout", rest)
    } else {
        ("fail", line.strip_prefix("FAIL")?)
    };
    let count = rest
        .strip_prefix(" (cached): ")?
        .strip_suffix(" selectors")?
        .parse::<usize>()
        .ok()?;
    Some(format!(
        "kiss test: lang_collapsed {} {label} {count}",
        lang.label()
    ))
}

pub(crate) fn strip_trailing_duration(body: &str) -> &str {
    let Some(idx) = body.rfind(" (") else {
        return body;
    };
    if body.ends_with(')') {
        &body[..idx]
    } else {
        body
    }
}

pub(crate) fn strip_ansi(message: &str) -> Cow<'_, str> {
    if !message.contains('\x1b') {
        return Cow::Borrowed(message);
    }
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(esc) = rest.find('\x1b') {
        out.push_str(&rest[..esc]);
        rest = &rest[esc..];
        let Some(csi) = rest.strip_prefix("\x1b[") else {
            out.push('\x1b');
            rest = &rest[1..];
            continue;
        };
        match csi.find('m') {
            Some(end) => rest = &csi[end + 1..],
            None => {
                out.push('\x1b');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

pub fn transcript_from_lines(lines: &[String]) -> Option<String> {
    if lines.is_empty() {
        return None;
    }
    let full = lines.join("\n");
    if full.len() <= WATCH_REPORT_BUDGET {
        Some(full)
    } else {
        Some(compact_watch_report(lines))
    }
}

fn is_watch_report_line(message: &str) -> bool {
    !strip_ansi(message.trim()).is_empty()
}

const WATCH_REPORT_BUDGET: usize = 200 * 1024;

fn compact_watch_report(lines: &[String]) -> String {
    let full = lines.join("\n");
    if full.len() <= WATCH_REPORT_BUDGET {
        return full;
    }
    let kept: Vec<&str> = lines
        .iter()
        .map(String::as_str)
        .filter(|line| {
            let text = strip_ansi(line.trim());
            !text.starts_with("PASS")
        })
        .collect();
    let compact = kept.join("\n");
    if compact.len() <= WATCH_REPORT_BUDGET {
        return compact;
    }
    let mut end = WATCH_REPORT_BUDGET.min(compact.len());
    while end > 0 && !compact.is_char_boundary(end) {
        end -= 1;
    }
    compact[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_report_capture_keeps_status_and_summary() {
        begin_watch_report_capture();
        record_watch_report_line("kiss test: Starting");
        record_watch_report_line("kiss test: request force=false force_bad=false metrics=false");
        record_watch_report_line("kiss test: Planning ...");
        record_watch_report_line("PASS: tests/a.py::t (0.01s)");
        record_watch_report_line("TIMEOUT: tests/b.py::t (5.00s)");
        record_watch_report_line("✓ 1 passed · 0 failed · 1 timed out · 1s total · 0s max pass");
        let report = take_watch_report_capture().expect("captured");
        assert!(report.contains("PASS: tests/a.py::t"));
        assert!(report.contains("TIMEOUT: tests/b.py::t"));
        assert!(report.contains("passed ·"));
        assert!(
            report.contains("Planning"),
            "client transcript must keep the standalone progress lines; report={report:?}"
        );
        assert!(report.contains("Starting"));
        assert!(report.contains("request"));
        assert!(take_watch_report_capture().is_none());
    }

    #[test]
    fn compact_watch_report_drops_pass_lines_then_truncates_over_budget() {
        let bulky = "Z".repeat(4096);
        let mut lines = Vec::new();
        for i in 0..80 {
            lines.push(format!("PASS: tests/p{i}.py::t (0.01s)"));
            lines.push(format!("TIMEOUT: tests/t{i}.py::t ({bulky})"));
        }
        let compact = compact_watch_report(&lines);
        assert!(
            compact.len() <= WATCH_REPORT_BUDGET,
            "compact len {}",
            compact.len()
        );
        assert!(
            !compact.contains("PASS:"),
            "over-budget compact must drop PASS lines first"
        );
        assert!(compact.contains("TIMEOUT:"));
        let via_transcript = transcript_from_lines(&lines).expect("transcript");
        assert!(via_transcript.len() <= WATCH_REPORT_BUDGET);
    }

    #[test]
    fn strip_ansi_removes_color_around_summary_icon() {
        let colored =
            "\x1b[31m✗\x1b[0m 11816 passed · 2 failed · 1 timed out · 69.33s total · 0s max pass";
        assert_eq!(
            strip_ansi(colored).as_ref(),
            "✗ 11816 passed · 2 failed · 1 timed out · 69.33s total · 0s max pass"
        );
        assert_eq!(strip_ansi("✓ 1 passed").as_ref(), "✓ 1 passed");
    }

    #[test]
    fn record_watch_report_line_tags_collapsed_with_progress_language() {
        begin_watch_report_capture();
        let _guard = ProgressLanguageGuard::enter(crate::Language::Rust);
        record_watch_report_line("PASS (cached): 2753 selectors");
        let lines = take_watch_report_lines().expect("lines");
        assert!(
            lines
                .iter()
                .any(|line| line == "kiss test: lang_collapsed rust pass 2753"),
            "{lines:?}"
        );
    }

    #[test]
    fn take_watch_report_parts_keeps_recorded_totals() {
        begin_watch_report_capture();
        record_watch_report_line("PASS (cached): 2633 selectors");
        record_watch_suite_totals(WatchSuiteTotals {
            passed: 11816,
            failed: 2,
            timed_out: 1,
            total_label: "69.33s".into(),
            max_pass_label: "0s".into(),
        });
        let (lines, totals) = take_watch_report_parts().expect("capture");
        assert!(lines.iter().any(|line| line.contains("2633")), "{lines:?}");
        let totals = totals.expect("totals");
        assert_eq!(totals.passed, 11816);
        assert_eq!(totals.failed, 2);
        assert_eq!(totals.timed_out, 1);
        assert!(take_watch_report_parts().is_none());
    }

    #[test]
    fn progress_lang_increments_named_and_collapsed() {
        begin_watch_report_capture();
        {
            let _guard = ProgressLanguageGuard::enter(crate::Language::Python);
            record_watch_report_line("PASS (cached): 2 selectors");
            record_watch_report_line("FAIL: tests/a.py::t (0.01s)");
        }
        record_watch_report_line("PASS (cached): 9 selectors");
        let taken = take_watch_report_taken().expect("taken");
        assert_eq!(taken.lang_passed, [2, 0]);
        assert_eq!(taken.lang_failed, [1, 0]);
        assert_eq!(taken.named.len(), 1);
        assert_eq!(taken.named[0].selector, "tests/a.py::t");
        assert_eq!(taken.named[0].outcome, WatchNamedOutcome::Fail);
    }

    #[test]
    fn timeout_without_progress_lang_is_named_from_selector_path() {
        begin_watch_report_capture();
        record_watch_report_line(
            "TIMEOUT: src/rpytest_runner/collector.rs::witness_collect_subprocess_paths (3.00s)",
        );
        record_watch_report_line("PASS: src/counts/tests.rs::test_violation_builder (0.02s)");
        let taken = take_watch_report_taken().expect("taken");
        assert_eq!(
            taken.named.len(),
            1,
            "PASS without lang must stay anonymous; {taken:?}"
        );
        assert_eq!(
            taken.named[0].selector,
            "src/rpytest_runner/collector.rs::witness_collect_subprocess_paths"
        );
        assert_eq!(taken.named[0].outcome, WatchNamedOutcome::Timeout);
        assert_eq!(taken.lang_timed_out, [0, 1]);
    }

    #[test]
    fn multiline_final_summary_footers_become_named() {
        begin_watch_report_capture();
        record_watch_report_line(
            "✗ 1 passed · 1 failed · 1 timed out · 1s total · 0s max pass\n\
             FAIL tests/a.py::test_a\n\
             TIMEOUT src/lib.rs::t_slow",
        );
        let taken = take_watch_report_taken().expect("taken");
        assert_eq!(taken.named.len(), 2, "{taken:?}");
        assert!(
            taken.named.iter().any(|row| {
                row.selector == "tests/a.py::test_a" && row.outcome == WatchNamedOutcome::Fail
            }),
            "{taken:?}"
        );
        assert!(
            taken.named.iter().any(|row| {
                row.selector == "src/lib.rs::t_slow" && row.outcome == WatchNamedOutcome::Timeout
            }),
            "{taken:?}"
        );
    }
}
