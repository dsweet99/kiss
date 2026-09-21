use std::collections::BTreeSet;

use super::LastReplies;
use crate::test_runner::force_bad::selector_in_target;

pub(super) fn slice_or_expand_recap(
    last: &LastReplies,
    output: &str,
    targets: &[String],
) -> Option<String> {
    slice_recap_for_targets(output, targets)
        .or_else(|| expand_collapsed_pass_targets(last, output, targets))
}

fn slice_recap_for_targets(output: &str, targets: &[String]) -> Option<String> {
    let mut kept = Vec::new();
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut timed_out = 0usize;
    for line in output.lines() {
        let Some((status, selector)) = recap_line_selector(line) else {
            continue;
        };
        if !targets.iter().any(|target| selector_in_target(selector, target)) {
            continue;
        }
        kept.push(format!("{status} (cached): {selector}"));
        match status {
            "PASS" => passed += 1,
            "FAIL" => failed += 1,
            "TIMEOUT" => timed_out += 1,
            _ => {}
        }
    }
    if kept.is_empty() {
        return None;
    }
    let icon = if failed + timed_out == 0 { "✓" } else { "✗" };
    kept.push(format!(
        "{icon} {passed} passed · {failed} failed · {timed_out} timed out · 0s total · 0s max pass"
    ));
    Some(kept.join("\n"))
}

fn expand_collapsed_pass_targets(
    last: &LastReplies,
    output: &str,
    targets: &[String],
) -> Option<String> {
    if !collapsed_problem_counts_are_named(output) {
        return None;
    }
    let selectors = cached_selectors_matching(last, targets);
    if selectors.is_empty() {
        return None;
    }
    let bad = named_problem_selectors(output);
    if selectors.iter().any(|selector| bad.contains(selector.as_str())) {
        return None;
    }
    let mut kept: Vec<String> = selectors
        .iter()
        .map(|selector| format!("PASS (cached): {selector}"))
        .collect();
    let passed = selectors.len();
    kept.push(format!(
        "✓ {passed} passed · 0 failed · 0 timed out · 0s total · 0s max pass"
    ));
    Some(kept.join("\n"))
}

fn collapsed_problem_counts_are_named(output: &str) -> bool {
    collapsed_status_count(output, "FAIL").unwrap_or(0) == named_status_count(output, "FAIL")
        && collapsed_status_count(output, "TIMEOUT").unwrap_or(0)
            == named_status_count(output, "TIMEOUT")
}

fn collapsed_status_count(output: &str, status: &str) -> Option<usize> {
    let prefix = format!("{status} (cached): ");
    output.lines().find_map(|line| {
        let rest = line.trim().strip_prefix(&prefix)?;
        let n = rest.strip_suffix(" selectors")?;
        n.parse().ok()
    })
}

fn named_status_count(output: &str, status: &str) -> usize {
    output
        .lines()
        .filter(|line| recap_line_selector(line).is_some_and(|(st, _)| st == status))
        .count()
}

fn named_problem_selectors(output: &str) -> BTreeSet<String> {
    output
        .lines()
        .filter_map(|line| {
            let (status, selector) = recap_line_selector(line)?;
            matches!(status, "FAIL" | "TIMEOUT").then(|| selector.to_string())
        })
        .collect()
}

fn cached_selectors_matching(last: &LastReplies, targets: &[String]) -> Vec<String> {
    let mut selectors = Vec::new();
    if let Some(python) = crate::test_runner::workspace_selector_cache::
        load_cached_python_workspace_selectors(&last.repo, &last.ignore, &last.python_extra)
    {
        selectors.extend(python);
    }
    if let Some(rust) = crate::test_runner::workspace_selector_cache::
        load_cached_rust_workspace_selectors(&last.repo, &last.ignore)
    {
        selectors.extend(rust);
    }
    selectors.retain(|selector| {
        targets
            .iter()
            .any(|target| selector_in_target(selector, target))
    });
    selectors.sort();
    selectors.dedup();
    selectors
}

fn recap_line_selector(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    for status in ["PASS", "FAIL", "TIMEOUT"] {
        let cached = format!("{status} (cached): ");
        if let Some(rest) = line.strip_prefix(&cached) {
            if is_collapsed_selector_count(rest) {
                return None;
            }
            return Some((status, strip_recap_duration(rest)));
        }
        let colon = format!("{status}: ");
        if let Some(rest) = line.strip_prefix(&colon) {
            if is_collapsed_selector_count(rest) {
                return None;
            }
            return Some((status, strip_recap_duration(rest)));
        }
        let spaced = format!("{status} ");
        if let Some(rest) = line.strip_prefix(&spaced) {
            if rest.starts_with('(') {
                continue;
            }
            return Some((status, strip_recap_duration(rest)));
        }
    }
    None
}

fn is_collapsed_selector_count(rest: &str) -> bool {
    rest.ends_with(" selectors")
        && rest
            .strip_suffix(" selectors")
            .is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()))
}

fn strip_recap_duration(selector: &str) -> &str {
    let Some(idx) = selector.rfind(" (") else {
        return selector;
    };
    let inner = &selector[idx + 2..];
    if inner.ends_with('s')
        && inner.len() > 1
        && inner[..inner.len() - 1]
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.')
    {
        return &selector[..idx];
    }
    selector
}
