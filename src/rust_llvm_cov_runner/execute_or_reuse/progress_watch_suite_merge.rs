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

struct CycleParse {
    named: BTreeMap<String, SuiteOutcome>,
    collapsed: [usize; 3],
    lang_collapsed: [[usize; 3]; 2],
    saw_lang: [bool; 2],
    summary: Option<(usize, usize, usize, String, String)>,
}

fn apply_parsed_line(suite: &mut WatchSuiteReport, line: &str, parsed: &mut CycleParse) {
    match parse_watch_line(line) {
        ParsedWatchLine::Named { selector, outcome } => {
            parsed.named.insert(selector, outcome);
        }
        ParsedWatchLine::Collapsed { outcome, count } => {
            parsed.collapsed[collapsed_index(outcome)] = count;
        }
        ParsedWatchLine::LangCollapsed {
            lang,
            outcome,
            count,
        } => {
            let i = lang_slot(lang);
            parsed.saw_lang[i] = true;
            parsed.lang_collapsed[i][collapsed_index(outcome)] += count;
        }
        ParsedWatchLine::Summary {
            passed,
            failed,
            timed_out,
            total,
            max_pass,
        } => {
            parsed.summary = Some((passed, failed, timed_out, total, max_pass));
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

fn merge_into(suite: &mut WatchSuiteReport, lines: &[String], unscoped: bool) {
    let mut parsed = CycleParse {
        named: BTreeMap::new(),
        collapsed: [0; 3],
        lang_collapsed: [[0; 3]; 2],
        saw_lang: [false; 2],
        summary: None,
    };
    for line in lines {
        apply_parsed_line(suite, line, &mut parsed);
    }
    let prior_passed = suite.passed();
    for (selector, outcome) in &parsed.named {
        apply_named(suite, selector.clone(), *outcome);
    }
    if unscoped && should_prune_absent_problems(parsed.summary.as_ref(), prior_passed, &parsed.named)
    {
        prune_absent_problems(suite, &parsed.named);
    }
    apply_anonymous_counts(
        suite,
        &parsed.named,
        parsed.collapsed,
        parsed.summary.as_ref().map(|s| s.0),
    );
    apply_lang_collapsed(suite, parsed.saw_lang, parsed.lang_collapsed);
    if let Some((_, _, _, total, max_pass)) = parsed.summary {
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
    suite.lang_failed = [0, 0];
    suite.lang_timed_out = [0, 0];
}

fn apply_lang_collapsed(suite: &mut WatchSuiteReport, saw: [bool; 2], counts: [[usize; 3]; 2]) {
    for (i, seen) in saw.into_iter().enumerate() {
        if !seen {
            continue;
        }
        suite.lang_passed[i] = counts[i][0];
        suite.lang_failed[i] = counts[i][1];
        suite.lang_timed_out[i] = counts[i][2];
    }
}

fn lang_slot(lang: crate::Language) -> usize {
    match lang {
        crate::Language::Python => 0,
        crate::Language::Rust => 1,
    }
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
    LangCollapsed {
        lang: crate::Language,
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
    if let Some(parsed) = parse_lang_collapsed(line) {
        return parsed;
    }
    parse_summary_line(line)
        .unwrap_or_else(|| parse_status_line(line).unwrap_or(ParsedWatchLine::Ignore))
}

fn parse_lang_collapsed(line: &str) -> Option<ParsedWatchLine> {
    let rest = line.strip_prefix("kiss test: lang_collapsed ")?;
    let mut parts = rest.split_whitespace();
    let lang = match parts.next()? {
        "python" | "py" => crate::Language::Python,
        "rust" | "rs" => crate::Language::Rust,
        _ => return None,
    };
    let outcome = match parts.next()? {
        "pass" => SuiteOutcome::Pass,
        "fail" => SuiteOutcome::Fail,
        "timeout" => SuiteOutcome::Timeout,
        _ => return None,
    };
    let count = parts.next()?.parse().ok()?;
    Some(ParsedWatchLine::LangCollapsed {
        lang,
        outcome,
        count,
    })
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
