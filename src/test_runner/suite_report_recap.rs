use kiss::rust_llvm_cov_runner::{WatchSuiteReport, WatchSuiteTotals};

use super::store::{StoredLangRecap, StoredNamed, StoredSuite};

pub(super) fn lang_index(lang: &str) -> Option<usize> {
    match lang {
        "python" => Some(0),
        "rust" => Some(1),
        _ => None,
    }
}

pub(super) fn language_recap_exit(suite: &StoredSuite, lang: &str) -> i32 {
    let Some(i) = lang_index(lang) else {
        return 0;
    };
    let named_timeout = suite
        .named
        .iter()
        .any(|row| row.lang == lang && row.outcome == "timeout");
    let named_fail = suite
        .named
        .iter()
        .any(|row| row.lang == lang && row.outcome == "fail");
    if suite.lang_timed_out[i] > 0 || named_timeout {
        124
    } else if suite.lang_failed[i] > 0 || named_fail {
        1
    } else {
        0
    }
}

pub(super) fn language_recap_body(suite: &StoredSuite, lang: &str) -> String {
    let Some(i) = lang_index(lang) else {
        return String::new();
    };
    let mut report = WatchSuiteReport::default();
    merge_named(
        &mut report,
        suite.named.iter().filter(|row| row.lang == lang),
    );
    report.apply_totals(&WatchSuiteTotals {
        passed: suite.lang_passed[i],
        failed: suite.lang_failed[i],
        timed_out: suite.lang_timed_out[i],
        total_label: suite.total_label.clone(),
        max_pass_label: suite.max_pass_label.clone(),
    });
    if language_recap_exit(suite, lang) == 0 {
        report.merge_lines(&["NO VIOLATIONS".into()]);
    }
    report.format()
}

pub(super) fn fill_language_recap(suite: &StoredSuite, lang: &str) -> Option<StoredLangRecap> {
    let has_counts = lang_index(lang).is_some_and(|i| {
        suite.lang_passed[i] + suite.lang_failed[i] + suite.lang_timed_out[i] > 0
    });
    if !has_counts && !suite.named.iter().any(|row| row.lang == lang) {
        return None;
    }
    let output = language_recap_body(suite, lang);
    if output.is_empty() {
        return None;
    }
    Some(StoredLangRecap {
        exit_code: language_recap_exit(suite, lang),
        output,
    })
}

pub(super) fn bilingual_recap(
    suite: &mut StoredSuite,
    live_totals: Option<&WatchSuiteTotals>,
    live_exit: i32,
    scoped: bool,
) -> Result<String, String> {
    let mut report = WatchSuiteReport::default();
    merge_named(&mut report, suite.named.iter());
    let totals = if scoped {
        lang_sum_totals(suite)
    } else {
        live_totals.cloned().unwrap_or_else(|| lang_sum_totals(suite))
    };
    report.apply_totals(&totals);
    if !scoped && live_exit == 0 {
        report.merge_lines(&["NO VIOLATIONS".into()]);
    }
    sync_anonymous(suite, &totals);
    if !scoped {
        suite.exit_code = live_exit;
        suite.total_label = totals.total_label.clone();
        suite.max_pass_label = totals.max_pass_label.clone();
        suite.gates_clean = live_exit == 0;
    }
    let output = report.format();
    if output.is_empty() {
        return Err("empty recap".to_string());
    }
    Ok(output)
}

pub(super) fn has_lang(suite: &StoredSuite, recap: Option<&StoredLangRecap>, lang: &str) -> bool {
    recap.is_some()
        || lang_index(lang).is_some_and(|i| {
            suite.lang_passed[i] + suite.lang_failed[i] + suite.lang_timed_out[i] > 0
        })
        || suite.named.iter().any(|row| row.lang == lang)
}

pub(super) fn totals_from_suite(suite: &StoredSuite) -> WatchSuiteTotals {
    let (named_pass, named_fail, named_timeout) = named_counts(suite);
    WatchSuiteTotals {
        passed: suite.anonymous_passed + named_pass,
        failed: suite.anonymous_failed + named_fail,
        timed_out: suite.anonymous_timed_out + named_timeout,
        total_label: suite.total_label.clone(),
        max_pass_label: suite.max_pass_label.clone(),
    }
}

pub(super) fn problem_sig(suite: &StoredSuite) -> (usize, usize, Vec<(String, String, String)>) {
    let failed = suite.lang_failed[0] + suite.lang_failed[1];
    let timed_out = suite.lang_timed_out[0] + suite.lang_timed_out[1];
    let mut rows: Vec<(String, String, String)> = suite
        .named
        .iter()
        .filter(|row| row.outcome == "fail" || row.outcome == "timeout")
        .map(|row| (row.lang.clone(), row.selector.clone(), row.outcome.clone()))
        .collect();
    rows.sort();
    (failed, timed_out, rows)
}

pub(super) fn scoped_suite_exit(suite: &StoredSuite) -> i32 {
    let timed_out = suite.lang_timed_out[0] + suite.lang_timed_out[1];
    let failed = suite.lang_failed[0] + suite.lang_failed[1];
    let named_timeout = suite.named.iter().any(|row| row.outcome == "timeout");
    let named_fail = suite.named.iter().any(|row| row.outcome == "fail");
    if timed_out > 0 || named_timeout {
        124
    } else if failed > 0 || named_fail {
        1
    } else {
        0
    }
}

fn merge_named<'a, I>(report: &mut WatchSuiteReport, rows: I)
where
    I: Iterator<Item = &'a StoredNamed>,
{
    let lines: Vec<String> = rows.filter_map(named_status_line).collect();
    if !lines.is_empty() {
        report.merge_lines(&lines);
    }
}

fn named_status_line(row: &StoredNamed) -> Option<String> {
    let label = match row.outcome.as_str() {
        "pass" => "PASS",
        "fail" => "FAIL",
        "timeout" => "TIMEOUT",
        _ => return None,
    };
    Some(format!("{label}: {}", row.selector))
}

fn lang_sum_totals(suite: &StoredSuite) -> WatchSuiteTotals {
    WatchSuiteTotals {
        passed: suite.lang_passed[0] + suite.lang_passed[1],
        failed: suite.lang_failed[0] + suite.lang_failed[1],
        timed_out: suite.lang_timed_out[0] + suite.lang_timed_out[1],
        total_label: suite.total_label.clone(),
        max_pass_label: suite.max_pass_label.clone(),
    }
}

fn named_counts(suite: &StoredSuite) -> (usize, usize, usize) {
    let mut pass = 0;
    let mut fail = 0;
    let mut timeout = 0;
    for row in &suite.named {
        match row.outcome.as_str() {
            "pass" => pass += 1,
            "fail" => fail += 1,
            "timeout" => timeout += 1,
            _ => {}
        }
    }
    (pass, fail, timeout)
}

fn sync_anonymous(suite: &mut StoredSuite, totals: &WatchSuiteTotals) {
    let (pass, fail, timeout) = named_counts(suite);
    suite.anonymous_passed = totals.passed.saturating_sub(pass);
    suite.anonymous_failed = totals.failed.saturating_sub(fail);
    suite.anonymous_timed_out = totals.timed_out.saturating_sub(timeout);
}
