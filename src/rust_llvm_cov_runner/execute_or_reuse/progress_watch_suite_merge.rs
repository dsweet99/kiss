use std::collections::BTreeMap;
use std::path::Path;

use super::{SuiteOutcome, WatchSuiteReport};

#[path = "progress_watch_suite_parse.rs"]
mod parse;
use parse::{ParsedWatchLine, parse_watch_line};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RustIdKind {
    Report,
    Logical,
}

impl WatchSuiteReport {
    pub fn retain_rust_selectors(
        &mut self,
        selectors: &[String],
        report_ids: &BTreeMap<String, String>,
    ) {
        self.named = std::mem::take(&mut self.named)
            .into_iter()
            .map(|(id, outcome)| (report_ids.get(&id).cloned().unwrap_or(id), outcome))
            .collect();
        let current: Vec<_> = selectors
            .iter()
            .map(|id| report_ids.get(id).unwrap_or(id).clone())
            .collect();
        self.retain_language_selectors(crate::Language::Rust, &current);
    }

    pub fn retain_language_selectors(&mut self, lang: crate::Language, selectors: &[String]) {
        let i = lang_slot(lang);
        self.inventory_empty[i] = selectors.is_empty();
        let current: std::collections::BTreeSet<&str> =
            selectors.iter().map(String::as_str).collect();
        self.named.retain(|selector, _| {
            let path = selector
                .split_once("::")
                .map_or(selector.as_str(), |(p, _)| p);
            let selector_lang =
                crate::Language::from_path(Path::new(path)).unwrap_or(crate::Language::Rust);
            selector_lang != lang || current.contains(selector.as_str())
        });
        self.inventory_named[i] = selectors
            .iter()
            .all(|selector| self.named.contains_key(selector));
        if self.inventory_named[i] {
            let mut named = [0; 3];
            for selector in &current {
                named[collapsed_index(self.named[*selector])] += 1;
            }
            self.anonymous_passed = self
                .anonymous_passed
                .saturating_sub(self.lang_passed[i].saturating_sub(named[0]));
            self.anonymous_failed = self
                .anonymous_failed
                .saturating_sub(self.lang_failed[i].saturating_sub(named[1]));
            self.anonymous_timed_out = self
                .anonymous_timed_out
                .saturating_sub(self.lang_timed_out[i].saturating_sub(named[2]));
            self.lang_passed[i] = 0;
            self.lang_failed[i] = 0;
            self.lang_timed_out[i] = 0;
        }
    }

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
    let prior_named = suite.named.len();
    for (selector, outcome) in &parsed.named {
        apply_named(suite, selector.clone(), *outcome);
    }
    if unscoped
        && should_prune_absent_problems(
            parsed.summary.as_ref(),
            prior_passed,
            prior_named,
            &parsed.named,
        )
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

fn prune_absent_problems(
    suite: &mut WatchSuiteReport,
    cycle_named: &BTreeMap<String, SuiteOutcome>,
) {
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
    if apply_rust_id_alias(suite, &selector, outcome) {
        return;
    }
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

fn apply_rust_id_alias(
    suite: &mut WatchSuiteReport,
    selector: &str,
    outcome: SuiteOutcome,
) -> bool {
    let Some(existing) = rust_alias_key(suite, selector) else {
        return false;
    };
    if classify_rust_id(selector).is_some_and(|(_, kind)| kind == RustIdKind::Report) {
        suite.named.remove(&existing);
        suite.named.insert(selector.to_string(), outcome);
    } else {
        suite.named.insert(existing, outcome);
    }
    true
}

fn rust_alias_key(suite: &WatchSuiteReport, selector: &str) -> Option<String> {
    let (name, kind) = classify_rust_id(selector)?;
    suite.named.keys().find_map(|key| {
        let (other_name, other_kind) = classify_rust_id(key)?;
        (other_name == name && other_kind != kind).then(|| key.clone())
    })
}

fn classify_rust_id(selector: &str) -> Option<(&str, RustIdKind)> {
    let (head, name) = selector.rsplit_once("::")?;
    let path_part = head.split_once("::").map_or(head, |(path, _)| path);
    match crate::Language::from_path(Path::new(path_part)) {
        Some(crate::Language::Python) => None,
        Some(crate::Language::Rust) => Some((name, RustIdKind::Report)),
        None => Some((name, RustIdKind::Logical)),
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
    suite
        .named
        .values()
        .filter(|item| **item == outcome)
        .count()
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
    prior_named: usize,
    cycle_named: &BTreeMap<String, SuiteOutcome>,
) -> bool {
    if let Some((passed, failed, timed_out, _, _)) = summary {
        if *failed == 0 && *timed_out == 0 {
            return *passed >= prior_passed && *passed >= prior_named.max(1);
        }
        return *passed + *failed + *timed_out >= prior_named.max(1);
    }
    cycle_named
        .values()
        .any(|outcome| matches!(outcome, SuiteOutcome::Fail | SuiteOutcome::Timeout))
        && cycle_named.len() >= prior_named.max(1)
}
