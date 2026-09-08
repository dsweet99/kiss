use std::collections::BTreeMap;

use super::progress_watch_report::strip_ansi_prefix;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SuiteOutcome {
    Pass,
    Fail,
    Timeout,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WatchSuiteReport {
    named: BTreeMap<String, SuiteOutcome>,
    anonymous_passed: usize,
    anonymous_failed: usize,
    anonymous_timed_out: usize,
    total_label: String,
    max_pass_label: String,
    violations: Vec<String>,
}

impl WatchSuiteReport {
    pub fn merge_lines(&mut self, lines: &[String]) {
        let mut cycle_named = BTreeMap::new();
        let mut collapsed = [0usize; 3];
        let mut summary = None;
        for line in lines {
            match parse_watch_line(line) {
                ParsedWatchLine::Named { selector, outcome } => {
                    cycle_named.insert(selector, outcome);
                }
                ParsedWatchLine::Collapsed { outcome, count } => {
                    collapsed[collapsed_index(outcome)] = count;
                }
                ParsedWatchLine::Summary {
                    passed,
                    failed,
                    timed_out,
                    total,
                    max_pass,
                } => {
                    summary = Some((passed, failed, timed_out, total, max_pass));
                }
                ParsedWatchLine::Violation(text) => {
                    if !self.violations.iter().any(|v| v == &text) {
                        self.violations.push(text);
                    }
                }
                ParsedWatchLine::Ignore => {}
            }
        }
        for (selector, outcome) in &cycle_named {
            self.apply_named(selector.clone(), *outcome);
        }
        self.apply_anonymous_counts(&cycle_named, collapsed, summary.as_ref().map(|s| s.0));
        if let Some((_, _, _, total, max_pass)) = summary {
            self.total_label = total;
            self.max_pass_label = max_pass;
        }
    }

    fn apply_named(&mut self, selector: String, outcome: SuiteOutcome) {
        if self.named.insert(selector, outcome).is_some() {
            return;
        }
        match outcome {
            SuiteOutcome::Pass if self.anonymous_passed > 0 => self.anonymous_passed -= 1,
            SuiteOutcome::Fail if self.anonymous_failed > 0 => self.anonymous_failed -= 1,
            SuiteOutcome::Timeout if self.anonymous_timed_out > 0 => self.anonymous_timed_out -= 1,
            SuiteOutcome::Fail | SuiteOutcome::Timeout if self.anonymous_passed > 0 => {
                self.anonymous_passed -= 1;
            }
            _ => {}
        }
    }

    fn apply_anonymous_counts(
        &mut self,
        cycle_named: &BTreeMap<String, SuiteOutcome>,
        collapsed: [usize; 3],
        summary_passed: Option<usize>,
    ) {
        if cycle_named.values().any(|o| *o == SuiteOutcome::Pass) {
            return;
        }
        let pass = collapsed[0].max(summary_passed.unwrap_or(0));
        let already = self.named_count(SuiteOutcome::Pass);
        if pass > already {
            self.anonymous_passed = pass - already;
        }
        self.take_collapsed_if_new(cycle_named, SuiteOutcome::Fail, collapsed[1]);
        self.take_collapsed_if_new(cycle_named, SuiteOutcome::Timeout, collapsed[2]);
    }

    fn take_collapsed_if_new(
        &mut self,
        cycle_named: &BTreeMap<String, SuiteOutcome>,
        outcome: SuiteOutcome,
        collapsed: usize,
    ) {
        if cycle_named.values().any(|item| *item == outcome) {
            return;
        }
        let already = self.named_count(outcome);
        if collapsed <= already {
            return;
        }
        let extra = collapsed - already;
        if outcome == SuiteOutcome::Fail {
            self.anonymous_failed = extra;
        } else {
            self.anonymous_timed_out = extra;
        }
    }

    fn named_count(&self, outcome: SuiteOutcome) -> usize {
        self.named.values().filter(|item| **item == outcome).count()
    }

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
        if lines.is_empty() && self.violations.is_empty() {
            return String::new();
        }
        lines.push(summary_line(self));
        lines.extend(failure_footers(self));
        lines.extend(self.violations.iter().cloned());
        lines.join("\n")
    }
}

fn collapsed_index(outcome: SuiteOutcome) -> usize {
    match outcome {
        SuiteOutcome::Pass => 0,
        SuiteOutcome::Fail => 1,
        SuiteOutcome::Timeout => 2,
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
    format!(
        "{icon} {} passed · {} failed · {} timed out · {total} total · {max_pass} max pass",
        suite.passed(),
        suite.failed(),
        suite.timed_out()
    )
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

enum ParsedWatchLine {
    Named {
        selector: String,
        outcome: SuiteOutcome,
    },
    Collapsed {
        outcome: SuiteOutcome,
        count: usize,
    },
    Summary {
        passed: usize,
        failed: usize,
        timed_out: usize,
        total: String,
        max_pass: String,
    },
    Violation(String),
    Ignore,
}

fn parse_watch_line(message: &str) -> ParsedWatchLine {
    let line = strip_ansi_prefix(message.trim());
    if line.contains("VIOLATION:test_coverage") {
        return ParsedWatchLine::Violation(line.to_string());
    }
    parse_summary_line(line).unwrap_or_else(|| parse_status_line(line).unwrap_or(ParsedWatchLine::Ignore))
}

fn parse_summary_line(line: &str) -> Option<ParsedWatchLine> {
    let rest = line
        .strip_prefix("✓ ")
        .or_else(|| line.strip_prefix("✗ "))?;
    if !rest.contains(" passed · ") {
        return None;
    }
    let parts: Vec<&str> = rest.split(" · ").collect();
    if parts.len() < 3 {
        return None;
    }
    Some(ParsedWatchLine::Summary {
        passed: parse_count_word(parts[0], "passed")?,
        failed: parse_count_word(parts[1], "failed")?,
        timed_out: parse_count_word(parts[2], "timed out")?,
        total: part_suffix(parts.get(3).copied(), " total"),
        max_pass: part_suffix(parts.get(4).copied(), " max pass"),
    })
}

fn parse_count_word(part: &str, word: &str) -> Option<usize> {
    part.strip_suffix(word)?.trim().parse().ok()
}

fn part_suffix(part: Option<&str>, suffix: &str) -> String {
    part.and_then(|text| text.strip_suffix(suffix))
        .unwrap_or("0s")
        .to_string()
}

fn parse_status_line(line: &str) -> Option<ParsedWatchLine> {
    let (outcome, rest) = if let Some(rest) = line.strip_prefix("PASS") {
        (SuiteOutcome::Pass, rest)
    } else if let Some(rest) = line.strip_prefix("TIMEOUT") {
        (SuiteOutcome::Timeout, rest)
    } else {
        (SuiteOutcome::Fail, line.strip_prefix("FAIL")?)
    };
    let body = rest
        .strip_prefix(" (cached): ")
        .or_else(|| rest.strip_prefix(": "))
        .or_else(|| rest.strip_prefix(' '))?;
    let selector = strip_trailing_duration(body);
    if let Some(count) = selector
        .strip_suffix(" selectors")
        .and_then(|n| n.parse::<usize>().ok())
    {
        return Some(ParsedWatchLine::Collapsed { outcome, count });
    }
    if selector.is_empty() {
        return None;
    }
    Some(ParsedWatchLine::Named {
        selector: selector.to_string(),
        outcome,
    })
}

fn strip_trailing_duration(body: &str) -> &str {
    let Some(idx) = body.rfind(" (") else {
        return body;
    };
    if body.ends_with(')') {
        &body[..idx]
    } else {
        body
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
}
