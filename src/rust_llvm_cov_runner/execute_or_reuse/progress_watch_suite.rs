use std::collections::BTreeMap;

#[path = "progress_watch_suite_merge.rs"]
mod merge;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SuiteOutcome {
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
    pub(crate) total_label: String,
    pub(crate) max_pass_label: String,
    pub(crate) violations: Vec<String>,
    pub(crate) gates_clean: bool,
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

    pub fn format(&self) -> String {
        let mut lines = status_lines(self);
        if lines.is_empty() && self.violations.is_empty() && !self.gates_clean {
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
}
