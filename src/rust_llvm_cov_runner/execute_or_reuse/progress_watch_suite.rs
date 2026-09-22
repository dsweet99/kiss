use std::collections::BTreeMap;

use super::progress_watch_report::WatchSuiteTotals;

#[path = "progress_watch_suite_merge.rs"]
mod merge;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SuiteOutcome {
    Pass,
    Fail,
    Timeout,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WatchSuiteReport {
    pub(crate) named: BTreeMap<String, SuiteOutcome>,
    pub(crate) anonymous_passed: usize,
    pub(crate) anonymous_failed: usize,
    pub(crate) anonymous_timed_out: usize,
    pub(crate) lang_passed: [usize; 2],
    pub(crate) lang_failed: [usize; 2],
    pub(crate) lang_timed_out: [usize; 2],
    pub(crate) total_label: String,
    pub(crate) max_pass_label: String,
    pub(crate) violations: Vec<String>,
    pub(crate) gates_clean: bool,
    pub(crate) inventory_empty: [bool; 2],
    pub(crate) inventory_named: [bool; 2],
}

impl WatchSuiteReport {
    pub fn passed(&self) -> usize {
        self.named_count(SuiteOutcome::Pass) + self.anonymous_passed
    }

    pub fn failed(&self) -> usize {
        self.named_count(SuiteOutcome::Fail) + self.anonymous_failed
    }

    pub fn timed_out(&self) -> usize {
        self.named_count(SuiteOutcome::Timeout) + self.anonymous_timed_out
    }

    pub fn test_exit_code(&self) -> i32 {
        i32::from(self.failed() + self.timed_out() > 0)
    }

    pub fn try_format_language(&self, lang: crate::Language) -> Option<(i32, String)> {
        let slice = language_slice(self, lang)?;
        Some((slice.recap_exit_code(), slice.format()))
    }

    pub fn apply_language_totals(
        &mut self,
        previous: &Self,
        lang: crate::Language,
        totals: &WatchSuiteTotals,
    ) {
        let Some(prior) = language_slice(previous, lang) else {
            return;
        };
        self.apply_totals(&WatchSuiteTotals {
            passed: previous.passed().saturating_sub(prior.passed()) + totals.passed,
            failed: previous.failed().saturating_sub(prior.failed()) + totals.failed,
            timed_out: previous.timed_out().saturating_sub(prior.timed_out()) + totals.timed_out,
            total_label: totals.total_label.clone(),
            max_pass_label: totals.max_pass_label.clone(),
        });
    }

    fn recap_exit_code(&self) -> i32 {
        if self.timed_out() > 0 {
            124
        } else if self.failed() > 0 || !self.violations.is_empty() {
            1
        } else {
            0
        }
    }

    pub fn format(&self) -> String {
        let mut lines = status_lines(self);
        if lines.is_empty() && self.violations.is_empty() && !self.gates_clean
            && !self.inventory_empty.iter().any(|empty| *empty)
        {
            return String::new();
        }
        lines.extend(failure_footers(self));
        if self.gates_clean && self.violations.is_empty() {
            lines.push("NO VIOLATIONS".into());
        }
        lines.extend(self.violations.iter().cloned());
        lines.push(summary_line(self));
        lines.join("\n")
    }

    fn named_count(&self, outcome: SuiteOutcome) -> usize {
        self.named.values().filter(|item| **item == outcome).count()
    }

    pub fn apply_totals(&mut self, totals: &WatchSuiteTotals) {
        self.anonymous_passed = totals.passed.saturating_sub(self.named_count(SuiteOutcome::Pass));
        self.anonymous_failed = totals.failed.saturating_sub(self.named_count(SuiteOutcome::Fail));
        self.anonymous_timed_out = totals
            .timed_out
            .saturating_sub(self.named_count(SuiteOutcome::Timeout));
        self.total_label = totals.total_label.clone();
        self.max_pass_label = totals.max_pass_label.clone();
    }
}

fn language_slice(suite: &WatchSuiteReport, lang: crate::Language) -> Option<WatchSuiteReport> {
    let i = match lang {
        crate::Language::Python => 0,
        crate::Language::Rust => 1,
    };
    let (lp, lf, lt) = (suite.lang_passed[i], suite.lang_failed[i], suite.lang_timed_out[i]);
    let has_lang = lp + lf + lt > 0;
    let untagged = suite.anonymous_passed + suite.anonymous_failed + suite.anonymous_timed_out > 0;
    let known_empty = suite.inventory_empty[i];
    if untagged && !has_lang && !known_empty && !suite.inventory_named[i] {
        return None;
    }
    let mut slice = WatchSuiteReport {
        inventory_empty: suite.inventory_empty,
        gates_clean: suite.gates_clean,
        violations: suite.violations.clone(),
        total_label: suite.total_label.clone(),
        max_pass_label: suite.max_pass_label.clone(),
        anonymous_passed: lp,
        anonymous_failed: lf,
        anonymous_timed_out: lt,
        ..WatchSuiteReport::default()
    };
    for (selector, outcome) in &suite.named {
        let path_part = selector.split_once("::").map_or(selector.as_str(), |(p, _)| p);
        if crate::Language::from_path(std::path::Path::new(path_part)) == Some(lang) {
            slice.named.insert(selector.clone(), *outcome);
        }
    }
    if slice.named.is_empty() && !has_lang && !known_empty {
        return None;
    }
    Some(slice)
}

fn outcome_label(outcome: SuiteOutcome) -> &'static str {
    match outcome {
        SuiteOutcome::Pass => "PASS",
        SuiteOutcome::Fail => "FAIL",
        SuiteOutcome::Timeout => "TIMEOUT",
    }
}

fn status_lines(suite: &WatchSuiteReport) -> Vec<String> {
    let list_named = suite.named.len() <= 64
        && suite.anonymous_passed == 0
        && suite.anonymous_failed == 0
        && suite.anonymous_timed_out == 0;
    if list_named {
        return suite
            .named
            .iter()
            .map(|(selector, outcome)| format!("{} (cached): {selector}", outcome_label(*outcome)))
            .collect();
    }
    let mut lines = Vec::new();
    push_collapsed(&mut lines, "PASS", suite.passed());
    push_collapsed(&mut lines, "FAIL", suite.failed());
    push_collapsed(&mut lines, "TIMEOUT", suite.timed_out());
    lines
}

fn summary_line(suite: &WatchSuiteReport) -> String {
    let icon = if suite.failed() + suite.timed_out() == 0 {
        "✓"
    } else {
        "✗"
    };
    let total = if suite.total_label.is_empty() {
        "0s"
    } else {
        suite.total_label.as_str()
    };
    let max_pass = if suite.max_pass_label.is_empty() {
        "0s"
    } else {
        suite.max_pass_label.as_str()
    };
    let mut line = format!(
        "{icon} {} passed · {} failed · {} timed out · {total} total · {max_pass} max pass",
        suite.passed(),
        suite.failed(),
        suite.timed_out()
    );
    append_violation_counts(&mut line, &suite.violations);
    line
}

fn append_violation_counts(line: &mut String, violations: &[String]) {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for text in violations {
        let Some(rest) = text.strip_prefix("VIOLATION:") else {
            continue;
        };
        let Some((kind, after)) = rest.split_once(':') else {
            continue;
        };
        let trimmed = after.trim_start();
        let n = trimmed
            .split_once(' ')
            .and_then(|(num, rest)| {
                let parsed = num.parse::<usize>().ok()?;
                (rest.starts_with("test(s)") || rest.starts_with("file(s)")).then_some(parsed)
            })
            .unwrap_or(1);
        if let Some((_, total)) = counts.iter_mut().find(|(name, _)| name == kind) {
            *total += n;
        } else {
            counts.push((kind.to_string(), n));
        }
    }
    for (kind, n) in counts {
        if n > 0 {
            line.push_str(&format!(" · {n} {kind}"));
        }
    }
}

fn failure_footers(suite: &WatchSuiteReport) -> Vec<String> {
    suite
        .named
        .iter()
        .filter_map(|(selector, outcome)| match outcome {
            SuiteOutcome::Fail => Some(format!("FAIL {selector}")),
            SuiteOutcome::Timeout => Some(format!("TIMEOUT {selector}")),
            SuiteOutcome::Pass => None,
        })
        .collect()
}

fn push_collapsed(lines: &mut Vec<String>, label: &str, count: usize) {
    if count > 0 {
        lines.push(format!("{label} (cached): {count} selectors"));
    }
}

pub fn merge_watch_exit(cycle_exit: i32, suite_exit: i32) -> i32 {
    if cycle_exit == 130 {
        130
    } else if suite_exit != 0 {
        suite_exit
    } else {
        cycle_exit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_totals_replace_only_refreshed_language_outcomes() {
        let mut previous = WatchSuiteReport::default();
        previous.merge_lines(&[
            "PASS (cached): 5 selectors".into(),
            "kiss test: lang_collapsed python pass 2".into(),
            "kiss test: lang_collapsed rust pass 3".into(),
            "FAIL (cached): 3 selectors".into(),
            "kiss test: lang_collapsed python fail 1".into(),
            "kiss test: lang_collapsed rust fail 2".into(),
            "TIMEOUT (cached): 3 selectors".into(),
            "kiss test: lang_collapsed python timeout 2".into(),
            "kiss test: lang_collapsed rust timeout 1".into(),
        ]);
        let mut current = WatchSuiteReport::default();
        current.apply_language_totals(&previous, crate::Language::Python, &WatchSuiteTotals {
            passed: 1,
            failed: 0,
            timed_out: 0,
            total_label: "1s".into(),
            max_pass_label: "1s".into(),
        });
        assert_eq!((current.passed(), current.failed(), current.timed_out()), (4, 2, 1));
    }

    #[test]
    fn language_totals_do_not_guess_unattributed_results() {
        let mut suite = WatchSuiteReport::default();
        suite.merge_lines(&["PASS (cached): 5 selectors".into()]);
        let previous = suite.clone();
        suite.apply_language_totals(&previous, crate::Language::Python, &WatchSuiteTotals {
            passed: 1,
            failed: 0,
            timed_out: 0,
            total_label: "1s".into(),
            max_pass_label: "1s".into(),
        });
        assert_eq!(suite, previous);
    }

    #[test]
    fn suite_recap_keeps_prior_failures_after_one_cached_pass() {
        let mut suite = WatchSuiteReport::default();
        suite.merge_lines(&[
            "PASS: tests/a.py::test_a (0.01s)".into(),
            "PASS: tests/b.py::test_b (0.01s)".into(),
            "FAIL: tests/c.py::test_c (0.01s)".into(),
            "✗ 2 passed · 1 failed · 0 timed out · 1s total · 0s max pass".into(),
            "FAIL tests/c.py::test_c".into(),
        ]);
        suite.merge_lines(&[
            "PASS (cached): tests/slow/test_ops_hogneato_sim_tuner_smoke_rust.py::test_ops_hogneato_sim_tuner_smoke_rust (0.46s)".into(),
            "✓ 1 passed · 0 failed · 0 timed out · 0.46s total · 0s max pass".into(),
        ]);
        let recap = suite.format();
        assert!(
            recap.contains("3 passed")
                && recap.contains("1 failed")
                && recap.contains("0 timed out"),
            "{recap}"
        );
        assert!(recap.contains("tests/c.py::test_c"), "{recap}");
        assert_eq!(suite.test_exit_code(), 1);
        let recap_at = recap.rfind("✗ 3 passed").expect("recap");
        let fail_at = recap.find("FAIL tests/c.py::test_c").expect("fail footer");
        assert!(fail_at < recap_at, "recap must be last:\n{recap}");
    }

    #[test]
    fn unscoped_green_cycle_drops_absent_failures() {
        let mut suite = WatchSuiteReport::default();
        suite.merge_lines(&[
            "PASS (cached): 4 selectors".into(),
            "FAIL: tests/gone.py::test_gone (0.01s)".into(),
            "✗ 4 passed · 1 failed · 0 timed out · 1s total · 0s max pass".into(),
            "FAIL tests/gone.py::test_gone".into(),
        ]);
        assert_eq!(suite.failed(), 1);
        suite.merge_unscoped_lines(&[
            "PASS (cached): 3355 selectors".into(),
            "NO VIOLATIONS".into(),
            "✓ 3355 passed · 0 failed · 0 timed out · 0.56s total · 0s max pass".into(),
        ]);
        assert_eq!(suite.failed(), 0);
        assert_eq!(suite.test_exit_code(), 0);
        let recap = suite.format();
        assert!(
            recap.contains("3355 passed")
                && recap.contains("0 failed")
                && !recap.contains("tests/gone.py::test_gone"),
            "{recap}"
        );
    }

    #[test]
    fn unscoped_cycle_keeps_failures_still_present() {
        let mut suite = WatchSuiteReport::default();
        suite.merge_lines(&[
            "FAIL: tests/a.py::test_a (0.01s)".into(),
            "FAIL: tests/b.py::test_b (0.01s)".into(),
            "✗ 0 passed · 2 failed · 0 timed out · 1s total · 0s max pass".into(),
        ]);
        suite.merge_unscoped_lines(&[
            "PASS (cached): 10 selectors".into(),
            "FAIL: tests/b.py::test_b (0.01s)".into(),
            "✗ 10 passed · 1 failed · 0 timed out · 1s total · 0s max pass".into(),
            "FAIL tests/b.py::test_b".into(),
        ]);
        assert_eq!(suite.failed(), 1);
        let recap = suite.format();
        assert!(recap.contains("tests/b.py::test_b"), "{recap}");
        assert!(!recap.contains("tests/a.py::test_a"), "{recap}");
    }

    #[test]
    fn unscoped_partial_green_keeps_prior_failures() {
        let mut suite = WatchSuiteReport::default();
        suite.merge_lines(&[
            "PASS: tests/a.py::test_a (0.01s)".into(),
            "PASS: tests/b.py::test_b (0.01s)".into(),
            "FAIL: tests/c.py::test_c (0.01s)".into(),
            "✗ 2 passed · 1 failed · 0 timed out · 1s total · 0s max pass".into(),
        ]);
        suite.merge_unscoped_lines(&[
            "PASS (cached): tests/slow/test_ops_hogneato_sim_tuner_smoke_rust.py::test_ops_hogneato_sim_tuner_smoke_rust (0.46s)".into(),
            "✓ 1 passed · 0 failed · 0 timed out · 0.46s total · 0s max pass".into(),
        ]);
        assert_eq!(suite.failed(), 1);
        assert!(suite.format().contains("tests/c.py::test_c"));
    }

    #[test]
    fn unscoped_incremental_green_keeps_prior_timeout() {
        let mut suite = WatchSuiteReport::default();
        suite.merge_lines(&[
            "PASS: tests/a.py::test_a (0.01s)".into(),
            "FAIL: tests/b.py::test_b (0.01s)".into(),
            "TIMEOUT: src/lib.rs::t_slow (0.01s)".into(),
            "✗ 1 passed · 1 failed · 1 timed out · 1s total · 0s max pass".into(),
        ]);
        suite.merge_unscoped_lines(&[
            "PASS: tests/a.py::test_a (0.01s)".into(),
            "✓ 1 passed · 0 failed · 0 timed out · 0.01s total · 0s max pass".into(),
        ]);
        let recap = suite.format();
        assert!(
            recap.contains("src/lib.rs::t_slow") && recap.contains("tests/b.py::test_b"),
            "{recap}"
        );
    }

    #[test]
    fn unscoped_incremental_fail_keeps_prior_timeout() {
        let mut suite = WatchSuiteReport::default();
        suite.merge_lines(&[
            "PASS: tests/a.py::test_a (0.01s)".into(),
            "PASS: tests/b.py::test_b (0.01s)".into(),
            "PASS: src/lib.rs::t_ok (0.01s)".into(),
            "TIMEOUT: src/lib.rs::t_slow (0.01s)".into(),
            "✗ 3 passed · 0 failed · 1 timed out · 1s total · 0s max pass".into(),
        ]);
        suite.merge_unscoped_lines(&[
            "FAIL: tests/a.py::test_a (0.01s)".into(),
            "✗ 0 passed · 1 failed · 0 timed out · 0.01s total · 0s max pass".into(),
            "FAIL tests/a.py::test_a".into(),
        ]);
        let recap = suite.format();
        assert!(
            recap.contains("src/lib.rs::t_slow") && recap.contains("tests/b.py::test_b"),
            "{recap}"
        );
    }

    #[test]
    fn suite_recap_uses_collapsed_pass_count_as_baseline() {
        let mut suite = WatchSuiteReport::default();
        suite.merge_lines(&[
            "PASS (cached): 4 selectors".into(),
            "FAIL (cached): 1 selectors".into(),
            "✗ 4 passed · 1 failed · 0 timed out · 2s total · 1s max pass".into(),
            "FAIL tests/c.py::test_c".into(),
        ]);
        assert_eq!(suite.passed(), 4);
        assert_eq!(suite.failed(), 1);
        suite.merge_lines(&[
            "PASS (cached): tests/a.py::test_a (0.01s)".into(),
            "✓ 1 passed · 0 failed · 0 timed out · 0.10s total · 0s max pass".into(),
        ]);
        assert_eq!(suite.passed(), 4);
        assert_eq!(suite.failed(), 1);
        assert_eq!(suite.test_exit_code(), 1);
    }

    #[test]
    fn suite_mixed_named_and_collapsed_uses_summary_total() {
        let mut suite = WatchSuiteReport::default();
        suite.merge_lines(&[
            "PASS (cached): tests/a.py::test_a (0.01s)".into(),
            "PASS (cached): tests/b.py::test_b (0.01s)".into(),
            "PASS (cached): 3156 selectors".into(),
            "✓ 3345 passed · 0 failed · 0 timed out · 0.76s total · 0s max pass".into(),
        ]);
        assert_eq!(suite.passed(), 3345);
        assert_eq!(suite.failed(), 0);
        let recap = suite.format();
        assert!(
            recap.contains("3345 passed") && recap.contains("0 failed"),
            "{recap}"
        );
    }

    #[test]
    fn suite_format_includes_no_violations_when_gates_clean() {
        let mut suite = WatchSuiteReport::default();
        suite.merge_lines(&[
            "PASS (cached): 3 selectors".into(),
            "NO VIOLATIONS".into(),
            "✓ 3 passed · 0 failed · 0 timed out · 0.1s total · 0s max pass".into(),
        ]);
        let recap = suite.format();
        assert!(
            recap.contains("NO VIOLATIONS") && recap.contains("3 passed"),
            "{recap}"
        );
        let clean_at = recap.find("NO VIOLATIONS").expect("clean");
        let summary_at = recap.find("✓ 3 passed").expect("summary");
        assert!(clean_at < summary_at, "{recap}");
    }

    #[test]
    fn suite_merges_timeout_collapsed_and_violation_counts() {
        let mut suite = WatchSuiteReport::default();
        suite.merge_lines(&[
            "TIMEOUT (cached): 2 selectors".into(),
            "VIOLATION:max_unit_test_seconds: 3 test(s) exceeded path-pattern time limits".into(),
            "VIOLATION:test_coverage:foo.py:1:foo: 0% covered".into(),
            "✗ 0 passed · 0 failed · 2 timed out · 3s total · 1s max pass".into(),
        ]);
        assert_eq!(suite.timed_out(), 2);
        let recap = suite.format();
        assert!(recap.contains("TIMEOUT (cached): 2 selectors"), "{recap}");
        assert!(recap.contains("· 3 max_unit_test_seconds"), "{recap}");
        assert!(recap.contains("· 1 test_coverage"), "{recap}");
        assert_eq!(suite.test_exit_code(), 1);
        assert_eq!(merge_watch_exit(0, 1), 1);
        assert_eq!(merge_watch_exit(130, 1), 130);
    }

    #[test]
    fn apply_totals_overrides_last_collapsed_pass() {
        let mut suite = WatchSuiteReport::default();
        suite.merge_unscoped_lines(&[
            "PASS (cached): 8557 selectors".into(),
            "kiss test: lang_collapsed python pass 8557".into(),
            "PASS (cached): 620 selectors".into(),
            "kiss test: lang_collapsed python pass 620".into(),
            "FAIL: tests/slow/ops/test_argus.py::test_argus_subscribe_counts_published_pings (1.00s)"
                .into(),
            "FAIL: tests/slow/ops/test_ops.py::test_ops_eval_measurement_model (3.24s)".into(),
            "TIMEOUT: tests/slow/ops/test_observability.py::test_observability (90.00s)".into(),
            "PASS (cached): 2633 selectors".into(),
            "kiss test: lang_collapsed rust pass 2633".into(),
        ]);
        assert_eq!(suite.passed(), 2633, "{}", suite.format());
        suite.apply_totals(&WatchSuiteTotals {
            passed: 11816,
            failed: 2,
            timed_out: 1,
            total_label: "69.33s".into(),
            max_pass_label: "0s".into(),
        });
        let recap = suite.format();
        assert_eq!(suite.passed(), 11816, "{recap}");
        assert!(
            recap.contains("11816 passed")
                && recap.contains("2 failed")
                && recap.contains("1 timed out")
                && recap.contains("69.33s total"),
            "{recap}"
        );
        assert!(
            !recap.contains("2633 passed"),
            "structured totals must not keep the last rust collapsed count; {recap}"
        );
    }

    #[test]
    fn merge_tty_colored_summary_keeps_bilingual_pass_count() {
        let mut suite = WatchSuiteReport::default();
        suite.merge_unscoped_lines(&[
            "PASS (cached): 8557 selectors".into(),
            "kiss test: lang_collapsed python pass 8557".into(),
            "PASS (cached): 620 selectors".into(),
            "kiss test: lang_collapsed python pass 620".into(),
            "FAIL: tests/slow/ops/test_argus.py::test_argus_subscribe_counts_published_pings (1.00s)"
                .into(),
            "FAIL: tests/slow/ops/test_ops.py::test_ops_eval_measurement_model (3.24s)".into(),
            "TIMEOUT: tests/slow/ops/test_observability.py::test_observability (90.00s)".into(),
            "PASS (cached): 2633 selectors".into(),
            "kiss test: lang_collapsed rust pass 2633".into(),
            "\x1b[31m✗\x1b[0m 11816 passed · 2 failed · 1 timed out · 69.33s total · 0s max pass\n\x1b[31mFAIL\x1b[0m tests/slow/ops/test_argus.py::test_argus_subscribe_counts_published_pings\n\x1b[31mTIMEOUT\x1b[0m tests/slow/ops/test_observability.py::test_observability\n\x1b[31mFAIL\x1b[0m tests/slow/ops/test_ops.py::test_ops_eval_measurement_model".into(),
        ]);
        let recap = suite.format();
        assert_eq!(suite.passed(), 11816, "{recap}");
        assert!(
            recap.contains("11816 passed")
                && recap.contains("2 failed")
                && recap.contains("1 timed out"),
            "{recap}"
        );
        assert!(
            !recap.contains("2633 passed"),
            "idle recap must not fall back to the last rust collapsed count; {recap}"
        );
        assert!(recap.contains("69.33s total"), "{recap}");
    }

    #[test]
    fn suite_format_uses_default_labels_when_summary_missing() {
        let mut suite = WatchSuiteReport::default();
        suite.merge_lines(&[
            "PASS (cached): 70 selectors".into(),
            "FAIL (cached): 1 selectors".into(),
            "TIMEOUT (cached): 1 selectors".into(),
        ]);
        let recap = suite.format();
        assert!(recap.contains("0s total"), "{recap}");
        assert!(recap.contains("0s max pass"), "{recap}");
        assert!(recap.contains("PASS (cached): 70 selectors"), "{recap}");
    }

    #[test]
    fn named_recap_aliases_rust_logical_ids_with_report_ids() {
        let mut suite = WatchSuiteReport::default();
        suite.merge_unscoped_lines(&[
            "PASS: test_lib.py::test_0 (0.01s)".into(),
            "PASS: src/lib.rs::test_0 (0.01s)".into(),
            "PASS: src/lib.rs::test_1 (0.01s)".into(),
            "✓ 3 passed · 0 failed · 0 timed out · 1s total · 0s max pass".into(),
        ]);
        suite.merge_unscoped_lines(&[
            "PASS (cached): test_lib.py::test_0".into(),
            "PASS (cached): tests::test_0".into(),
            "PASS (cached): tests::test_1".into(),
            "✓ 3 passed · 0 failed · 0 timed out · 0.05s total · 0s max pass".into(),
        ]);
        assert_eq!(suite.passed(), 3, "recap={}", suite.format());
        let recap = suite.format();
        assert!(recap.contains("3 passed"), "{recap}");
        assert!(recap.contains("src/lib.rs::test_0"), "{recap}");
        assert!(!recap.contains("tests::test_0"), "{recap}");
        let (_, rs) = suite
            .try_format_language(crate::Language::Rust)
            .expect("rust slice");
        assert!(rs.contains("2 passed"), "{rs}");
    }

    #[test]
    fn try_format_language_splits_named_selectors() {
        let mut suite = WatchSuiteReport::default();
        suite.merge_lines(&[
            "PASS: tests/a.py::test_a (0.01s)".into(),
            "PASS: src/lib.rs::a_ok (0.01s)".into(),
            "✓ 2 passed · 0 failed · 0 timed out · 1s total · 0s max pass".into(),
        ]);
        let (py_code, py) = suite
            .try_format_language(crate::Language::Python)
            .expect("python slice");
        let (rs_code, rs) = suite
            .try_format_language(crate::Language::Rust)
            .expect("rust slice");
        assert_eq!(py_code, 0);
        assert_eq!(rs_code, 0);
        assert!(py.contains("tests/a.py::test_a") && !py.contains("src/lib.rs::a_ok"), "{py}");
        assert!(rs.contains("src/lib.rs::a_ok") && !rs.contains("tests/a.py::test_a"), "{rs}");
        assert!(suite.try_format_language(crate::Language::Python).is_some());
        suite.anonymous_passed = 3;
        assert!(suite.try_format_language(crate::Language::Rust).is_none());
    }

    #[test]
    fn try_format_language_keeps_collapsed_counts_when_named_exist() {
        let mut suite = WatchSuiteReport::default();
        suite.merge_lines(&[
            "PASS (cached): 8557 selectors".into(),
            "kiss test: lang_collapsed python pass 8557".into(),
            "PASS (cached): 620 selectors".into(),
            "kiss test: lang_collapsed python pass 620".into(),
            "FAIL: tests/slow/ops/test_ops.py::test_ops_eval_measurement_model (3.24s)".into(),
            "TIMEOUT: tests/slow/ops/test_observability.py::test_observability (90.00s)".into(),
            "PASS (cached): 2633 selectors".into(),
            "kiss test: lang_collapsed rust pass 2633".into(),
            "✗ 11815 passed · 2 failed · 2 timed out · 96s total · 8s max pass".into(),
        ]);
        let (py_code, py) = suite
            .try_format_language(crate::Language::Python)
            .expect("python slice");
        let (rs_code, rs) = suite
            .try_format_language(crate::Language::Rust)
            .expect("rust slice");
        assert_eq!(py_code, 124);
        assert_eq!(rs_code, 0);
        assert!(
            py.contains("9177 passed") && py.contains("1 failed") && py.contains("1 timed out"),
            "python slice must keep collapsed passes plus named problems; py={py}"
        );
        assert!(
            py.contains("test_ops_eval_measurement_model")
                && !py.contains("2633"),
            "python slice={py}"
        );
        assert!(
            rs.contains("2633 passed") && !rs.contains("8557") && !rs.contains("test_ops"),
            "rust slice={rs}"
        );
    }

    #[test]
    fn try_format_language_uses_lang_collapsed_tags() {
        let mut suite = WatchSuiteReport::default();
        suite.merge_lines(&[
            "PASS (cached): 8634 selectors".into(),
            "kiss test: lang_collapsed python pass 8634".into(),
            "PASS (cached): 2753 selectors".into(),
            "kiss test: lang_collapsed rust pass 2753".into(),
            "✓ 11387 passed · 0 failed · 0 timed out · 1s total · 0s max pass".into(),
        ]);
        let (py_code, py) = suite
            .try_format_language(crate::Language::Python)
            .expect("python collapsed");
        let (rs_code, rs) = suite
            .try_format_language(crate::Language::Rust)
            .expect("rust collapsed");
        assert_eq!(py_code, 0);
        assert_eq!(rs_code, 0);
        assert!(
            py.contains("8634") && !py.contains("2753"),
            "python slice={py}"
        );
        assert!(
            rs.contains("2753") && !rs.contains("8634"),
            "rust slice={rs}"
        );
    }

    #[test]
    fn try_format_language_sums_same_lang_collapsed_pass_groups() {
        let mut suite = WatchSuiteReport::default();
        suite.merge_lines(&[
            "PASS (cached): 7861 selectors".into(),
            "kiss test: lang_collapsed python pass 7861".into(),
            "PASS (cached): 773 selectors".into(),
            "kiss test: lang_collapsed python pass 773".into(),
            "PASS (cached): 2753 selectors".into(),
            "kiss test: lang_collapsed rust pass 2753".into(),
            "✓ 11387 passed · 0 failed · 0 timed out · 1s total · 0s max pass".into(),
        ]);
        let (_, py) = suite
            .try_format_language(crate::Language::Python)
            .expect("python collapsed");
        let (_, rs) = suite
            .try_format_language(crate::Language::Rust)
            .expect("rust collapsed");
        assert!(
            py.contains("8634") && !py.contains("773") && !py.contains("7861"),
            "python slice must be 7861+773=8634; py={py}"
        );
        assert!(
            rs.contains("2753") && !rs.contains("8634"),
            "rust slice={rs}"
        );
    }

    #[test]
    fn try_format_language_exit_follows_slice_violations_not_other_lang_fails() {
        let mut suite = WatchSuiteReport::default();
        suite.merge_lines(&[
            "PASS (cached): 8634 selectors".into(),
            "kiss test: lang_collapsed python pass 8634".into(),
            "FAIL: src/lib.rs::a_ok (0.01s)".into(),
            "kiss test: lang_collapsed rust fail 1".into(),
            "✗ 8634 passed · 1 failed · 0 timed out · 1s total · 0s max pass".into(),
        ]);
        let (py_code, _) = suite
            .try_format_language(crate::Language::Python)
            .expect("python");
        let (rs_code, _) = suite
            .try_format_language(crate::Language::Rust)
            .expect("rust");
        assert_eq!(py_code, 0);
        assert_eq!(rs_code, 1);
        suite.merge_lines(&[
            "VIOLATION:test_coverage: codebase coverage 50% below 90% threshold".into(),
        ]);
        let (py_code, py) = suite
            .try_format_language(crate::Language::Python)
            .expect("python with gate");
        assert_eq!(py_code, 1);
        assert!(py.contains("VIOLATION:test_coverage:"), "{py}");
    }
}
