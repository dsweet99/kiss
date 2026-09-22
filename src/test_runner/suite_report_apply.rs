use kiss::rust_llvm_cov_runner::{WatchNamed, WatchNamedOutcome};

use super::digest::SuiteDigests;
use super::recap::{
    bilingual_recap, fill_language_recap, has_lang, lang_has_work, language_recap_body,
    language_recap_exit, lang_index, problem_sig, scoped_suite_exit, totals_from_suite,
};
use super::store::{DurableSuiteRecap, StoredLangRecap, StoredNamed, StoredRecap, StoredSuite};
use super::KissTestReport;

pub(super) fn apply_unscoped(
    stored: &mut DurableSuiteRecap,
    report: &KissTestReport,
    digests: &SuiteDigests,
) -> Result<(), String> {
    stored.digest_all = digests.all.clone();
    stored.digest_python = digests.python.clone();
    stored.digest_rust = digests.rust.clone();
    stored.suite.named = live_named(&report.named);
    stored.suite.violations = extract_violations(&report.lines);
    let kept_stored_lang = keep_unattributed_lang_counts(stored, report);
    let live_totals = if kept_stored_lang {
        None
    } else {
        report.totals.as_ref()
    };
    let output = bilingual_recap(&mut stored.suite, live_totals, report.exit_code, false)?;
    stored.recaps.all = Some(StoredRecap { output });
    stored.recaps.python = fill_language_recap(&stored.suite, "python");
    stored.recaps.rust = fill_language_recap(&stored.suite, "rust");
    Ok(())
}

fn keep_unattributed_lang_counts(stored: &mut DurableSuiteRecap, report: &KissTestReport) -> bool {
    let mut lang_passed = report.lang_passed;
    let mut lang_failed = report.lang_failed;
    let mut lang_timed_out = report.lang_timed_out;
    let mut kept = false;
    for lang in ["python", "rust"] {
        let Some(i) = lang_index(lang) else {
            continue;
        };
        if scoped_report_has_lang(report, lang) || !lang_has_work(&stored.suite, lang) {
            continue;
        }
        lang_passed[i] = stored.suite.lang_passed[i];
        lang_failed[i] = stored.suite.lang_failed[i];
        lang_timed_out[i] = stored.suite.lang_timed_out[i];
        kept = true;
    }
    stored.suite.lang_passed = lang_passed;
    stored.suite.lang_failed = lang_failed;
    stored.suite.lang_timed_out = lang_timed_out;
    kept
}

pub(super) fn scoped_report_has_lang(report: &KissTestReport, lang: &str) -> bool {
    lang_index(lang).is_some_and(|i| {
        report.lang_passed[i] + report.lang_failed[i] + report.lang_timed_out[i] > 0
    }) || report.named.iter().any(|row| row.lang.label() == lang)
}

pub(super) fn apply_scoped(
    stored: &mut DurableSuiteRecap,
    report: &KissTestReport,
    lang: &str,
    digests: &SuiteDigests,
) -> Result<(), String> {
    if !scoped_report_has_lang(report, lang) {
        return Ok(());
    }
    let old_sig = problem_sig(&stored.suite);
    let old_exit = stored.suite.exit_code;
    replace_lang_named(&mut stored.suite, lang, &report.named);
    if let Some(i) = lang_index(lang) {
        stored.suite.lang_passed[i] = report.lang_passed[i];
        stored.suite.lang_failed[i] = report.lang_failed[i];
        stored.suite.lang_timed_out[i] = report.lang_timed_out[i];
    }
    merge_scoped_violations(&mut stored.suite, report, lang);
    set_lang_digest(stored, lang, digests);
    if let Some(body) = nonempty_body(&stored.suite, lang) {
        set_lang_recap(
            stored,
            lang,
            language_recap_exit(&stored.suite, lang),
            body,
        );
    }
    let output = bilingual_recap(&mut stored.suite, None, report.exit_code, true)?;
    if !stored.digest_all.is_empty() {
        stored.digest_all = digests.all.clone();
    }
    stored.recaps.all = Some(StoredRecap { output });
    if old_sig != problem_sig(&stored.suite) {
        stored.suite.exit_code = scoped_suite_exit(&stored.suite);
    } else {
        stored.suite.exit_code = old_exit;
    }
    Ok(())
}

pub(super) fn replay_all(stored: &DurableSuiteRecap, digest_all: &str) -> Option<KissTestReport> {
    if stored.digest_all != digest_all {
        return None;
    }
    if vacuous_lang_recap(stored, "python") || vacuous_lang_recap(stored, "rust") {
        return None;
    }
    let output = match &stored.recaps.all {
        Some(recap) => recap.output.clone(),
        None => bilingual_recap(&mut stored.suite.clone(), None, stored.suite.exit_code, true).ok()?,
    };
    if output.is_empty() {
        return None;
    }
    Some(replay_report(
        stored.suite.exit_code,
        output,
        totals_from_suite(&stored.suite),
    ))
}

pub(super) fn replay_lang(
    stored: &DurableSuiteRecap,
    lang: &str,
    digest: &str,
) -> Option<KissTestReport> {
    if stored_lang_digest(stored, lang)? != digest {
        return None;
    }
    if vacuous_lang_recap(stored, lang) {
        return None;
    }
    let recap = stored_lang_recap(stored, lang);
    if !has_lang(&stored.suite, recap, lang) {
        return None;
    }
    let (exit_code, output) = match recap {
        Some(recap) => (recap.exit_code, recap.output.clone()),
        None => {
            let output = language_recap_body(&stored.suite, lang);
            if output.is_empty() {
                return None;
            }
            (language_recap_exit(&stored.suite, lang), output)
        }
    };
    Some(replay_report(
        exit_code,
        output,
        totals_from_suite(&filter_suite_lang(&stored.suite, lang)),
    ))
}

fn live_named(named: &[WatchNamed]) -> Vec<StoredNamed> {
    named
        .iter()
        .map(|row| StoredNamed {
            lang: row.lang.label().to_string(),
            selector: row.selector.clone(),
            outcome: outcome_label(row.outcome).to_string(),
        })
        .collect()
}

fn replace_lang_named(suite: &mut StoredSuite, lang: &str, live: &[WatchNamed]) {
    suite.named.retain(|row| row.lang != lang);
    suite.named.extend(
        live_named(live)
            .into_iter()
            .filter(|row| row.lang == lang),
    );
}

fn set_lang_digest(stored: &mut DurableSuiteRecap, lang: &str, digests: &SuiteDigests) {
    match lang {
        "python" => stored.digest_python = digests.python.clone(),
        "rust" => stored.digest_rust = digests.rust.clone(),
        _ => {}
    }
}

fn set_lang_recap(stored: &mut DurableSuiteRecap, lang: &str, exit_code: i32, output: String) {
    let recap = StoredLangRecap { exit_code, output };
    match lang {
        "python" => stored.recaps.python = Some(recap),
        "rust" => stored.recaps.rust = Some(recap),
        _ => {}
    }
}

fn nonempty_body(suite: &StoredSuite, lang: &str) -> Option<String> {
    let output = language_recap_body(suite, lang);
    (!output.is_empty()).then_some(output)
}

fn vacuous_lang_recap(stored: &DurableSuiteRecap, lang: &str) -> bool {
    stored_lang_recap(stored, lang).is_some() && !lang_has_work(&stored.suite, lang)
}

fn stored_lang_digest<'a>(stored: &'a DurableSuiteRecap, lang: &str) -> Option<&'a str> {
    match lang {
        "python" => Some(stored.digest_python.as_str()),
        "rust" => Some(stored.digest_rust.as_str()),
        _ => None,
    }
}

fn stored_lang_recap<'a>(
    stored: &'a DurableSuiteRecap,
    lang: &str,
) -> Option<&'a StoredLangRecap> {
    match lang {
        "python" => stored.recaps.python.as_ref(),
        "rust" => stored.recaps.rust.as_ref(),
        _ => None,
    }
}

fn filter_suite_lang(suite: &StoredSuite, lang: &str) -> StoredSuite {
    let mut slice = suite.clone();
    slice.named.retain(|row| row.lang == lang);
    if let Some(i) = lang_index(lang) {
        let other = 1 - i;
        slice.lang_passed[other] = 0;
        slice.lang_failed[other] = 0;
        slice.lang_timed_out[other] = 0;
        slice.anonymous_passed = slice.lang_passed[i];
        slice.anonymous_failed = slice.lang_failed[i];
        slice.anonymous_timed_out = slice.lang_timed_out[i];
        let pass = slice.named.iter().filter(|row| row.outcome == "pass").count();
        let fail = slice.named.iter().filter(|row| row.outcome == "fail").count();
        let timeout = slice
            .named
            .iter()
            .filter(|row| row.outcome == "timeout")
            .count();
        slice.anonymous_passed = slice.anonymous_passed.saturating_sub(pass);
        slice.anonymous_failed = slice.anonymous_failed.saturating_sub(fail);
        slice.anonymous_timed_out = slice.anonymous_timed_out.saturating_sub(timeout);
    }
    slice
}

fn replay_report(exit_code: i32, output: String, totals: kiss::rust_llvm_cov_runner::WatchSuiteTotals) -> KissTestReport {
    KissTestReport {
        exit_code,
        output: Some(output),
        totals: Some(totals),
        ..KissTestReport::default()
    }
}

fn extract_violations(lines: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for line in lines {
        if line.contains("VIOLATION:") && !out.contains(line) {
            out.push(line.clone());
        }
    }
    out
}

fn merge_scoped_violations(suite: &mut StoredSuite, report: &KissTestReport, lang: &str) {
    let scoped = extract_violations(&report.lines);
    let other = match lang {
        "python" => "rust",
        "rust" => "python",
        _ => return,
    };
    if !lang_has_work(suite, other) {
        suite.gates_clean = report.exit_code == 0 && scoped.is_empty();
        suite.violations = scoped;
        return;
    }
    if scoped.is_empty() {
        return;
    }
    for line in scoped {
        if !suite.violations.contains(&line) {
            suite.violations.push(line);
        }
    }
    suite.gates_clean = false;
}

fn outcome_label(outcome: WatchNamedOutcome) -> &'static str {
    match outcome {
        WatchNamedOutcome::Pass => "pass",
        WatchNamedOutcome::Fail => "fail",
        WatchNamedOutcome::Timeout => "timeout",
    }
}
