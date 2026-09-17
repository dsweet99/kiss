use std::collections::BTreeMap;

use super::super::progress_watch_report::strip_ansi_prefix;
use super::{SuiteOutcome, WatchSuiteReport};

impl WatchSuiteReport {
    pub fn merge_lines(&mut self, lines: &[String]) {
        merge_into(self, lines, false);
    }

    pub fn merge_unscoped_lines(&mut self, lines: &[String]) {
        merge_into(self, lines, true);
    }
}

fn merge_into(suite: &mut WatchSuiteReport, lines: &[String], unscoped: bool) {
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
                suite.gates_clean = false;
                if !suite.violations.iter().any(|v| v == &text) {
                    suite.violations.push(text);
                }
            }
            ParsedWatchLine::NoViolations => {
                suite.gates_clean = true;
                suite.violations.clear();
            }
            ParsedWatchLine::Ignore => {}
        }
    }
    let prior_passed = suite.passed();
    for (selector, outcome) in &cycle_named {
        apply_named(suite, selector.clone(), *outcome);
    }
    if unscoped && should_prune_absent_problems(summary.as_ref(), prior_passed, &cycle_named) {
        prune_absent_problems(suite, &cycle_named);
    }
    apply_anonymous_counts(suite, &cycle_named, collapsed, summary.as_ref().map(|s| s.0));
    if let Some((_, _, _, total, max_pass)) = summary {
        suite.total_label = total;
        suite.max_pass_label = max_pass;
    }
}

fn prune_absent_problems(suite: &mut WatchSuiteReport, cycle_named: &BTreeMap<String, SuiteOutcome>) {
    suite.named.retain(|selector, outcome| match outcome {
        SuiteOutcome::Pass => true,
        SuiteOutcome::Fail | SuiteOutcome::Timeout => cycle_named.contains_key(selector),
    });
    suite.anonymous_failed = 0;
    suite.anonymous_timed_out = 0;
}

fn apply_named(suite: &mut WatchSuiteReport, selector: String, outcome: SuiteOutcome) {
    if suite.named.insert(selector, outcome).is_some() {
        return;
    }
    match outcome {
        SuiteOutcome::Pass if suite.anonymous_passed > 0 => suite.anonymous_passed -= 1,
        SuiteOutcome::Fail if suite.anonymous_failed > 0 => suite.anonymous_failed -= 1,
        SuiteOutcome::Timeout if suite.anonymous_timed_out > 0 => suite.anonymous_timed_out -= 1,
        SuiteOutcome::Fail | SuiteOutcome::Timeout if suite.anonymous_passed > 0 => {
            suite.anonymous_passed -= 1;
        }
        _ => {}
    }
}

fn apply_anonymous_counts(
    suite: &mut WatchSuiteReport,
    cycle_named: &BTreeMap<String, SuiteOutcome>,
    collapsed: [usize; 3],
    summary_passed: Option<usize>,
) {
    let pass = collapsed[0].max(summary_passed.unwrap_or(0));
    let target = pass.max(suite.passed());
    let already = named_count(suite, SuiteOutcome::Pass);
    suite.anonymous_passed = target.saturating_sub(already);
    take_collapsed_if_new(suite, cycle_named, SuiteOutcome::Fail, collapsed[1]);
    take_collapsed_if_new(suite, cycle_named, SuiteOutcome::Timeout, collapsed[2]);
}

fn take_collapsed_if_new(
    suite: &mut WatchSuiteReport,
    cycle_named: &BTreeMap<String, SuiteOutcome>,
    outcome: SuiteOutcome,
    collapsed: usize,
) {
    if cycle_named.values().any(|item| *item == outcome) {
        return;
    }
    let already = named_count(suite, outcome);
    if collapsed <= already {
        return;
    }
    let extra = collapsed - already;
    if outcome == SuiteOutcome::Fail {
        suite.anonymous_failed = extra;
    } else {
        suite.anonymous_timed_out = extra;
    }
}

fn named_count(suite: &WatchSuiteReport, outcome: SuiteOutcome) -> usize {
    suite.named.values().filter(|item| **item == outcome).count()
}

fn collapsed_index(outcome: SuiteOutcome) -> usize {
    match outcome {
        SuiteOutcome::Pass => 0,
        SuiteOutcome::Fail => 1,
        SuiteOutcome::Timeout => 2,
    }
}

fn should_prune_absent_problems(
    summary: Option<&(usize, usize, usize, String, String)>,
    prior_passed: usize,
    cycle_named: &BTreeMap<String, SuiteOutcome>,
) -> bool {
    if let Some((passed, failed, timed_out, _, _)) = summary {
        if *failed == 0 && *timed_out == 0 {
            return *passed >= prior_passed;
        }
        return true;
    }
    cycle_named
        .values()
        .any(|outcome| matches!(outcome, SuiteOutcome::Fail | SuiteOutcome::Timeout))
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
    NoViolations,
    Ignore,
}

fn parse_watch_line(message: &str) -> ParsedWatchLine {
    let line = strip_ansi_prefix(message.trim());
    if line.contains("VIOLATION:") {
        return ParsedWatchLine::Violation(line.to_string());
    }
    if line == "NO VIOLATIONS" {
        return ParsedWatchLine::NoViolations;
    }
    parse_summary_line(line)
        .unwrap_or_else(|| parse_status_line(line).unwrap_or(ParsedWatchLine::Ignore))
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
