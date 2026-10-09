#![cfg_attr(not(test), allow(dead_code))]
use kiss::watch_report::{WatchNamed, WatchNamedOutcome, WatchSuiteTotals};

use super::RunTestCmdArgs;
#[cfg(test)]
use super::RunTestOnceOutcome;
use crate::test_runner::target_request::{
    EffectiveStatus, TargetReport, request_from_run_args, to_compat_invocation,
};

#[allow(dead_code)]
#[path = "suite_report.rs"]
mod suite_report;

pub(crate) const EXIT_INTERRUPTED: i32 = 130;

#[allow(dead_code)]
pub(crate) const KISS_TEST_ALLOW_REFRESH: bool = false;

pub(crate) fn kiss_report_from_target(report: &TargetReport) -> Option<KissTestReport> {
    let output = crate::test_runner::target_request::official_report_text(report);
    let mut lang_passed = [0usize; 2];
    let mut lang_failed = [0usize; 2];
    let mut lang_timed_out = [0usize; 2];
    let mut named = Vec::new();
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut timed_out = 0usize;
    for row in &report.rows {
        let outcome = match row.effective {
            EffectiveStatus::Pass => {
                passed += 1;
                WatchNamedOutcome::Pass
            }
            EffectiveStatus::Fail => {
                failed += 1;
                WatchNamedOutcome::Fail
            }
            EffectiveStatus::Timeout => {
                timed_out += 1;
                WatchNamedOutcome::Timeout
            }
        };
        let language = row.language;
        let i = language.index();
        match outcome {
            WatchNamedOutcome::Pass => lang_passed[i] += 1,
            WatchNamedOutcome::Fail => lang_failed[i] += 1,
            WatchNamedOutcome::Timeout => lang_timed_out[i] += 1,
        }
        named.push(WatchNamed {
            lang: language,
            selector: row.selector.clone(),
            outcome,
        });
    }
    Some(KissTestReport {
        exit_code: report.exit_code,
        output: Some(output.clone()),
        lines: output.lines().map(str::to_string).collect(),
        totals: Some(WatchSuiteTotals {
            passed,
            failed,
            timed_out,
            total_label: format!("{} total", passed + failed + timed_out),
            max_pass_label: String::new(),
        }),
        error: None,
        interrupted: false,
        engine_aborted: false,
        named,
        lang_passed,
        lang_failed,
        lang_timed_out,
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct KissTestReport {
    pub exit_code: i32,
    pub output: Option<String>,
    pub lines: Vec<String>,
    pub totals: Option<WatchSuiteTotals>,
    pub error: Option<String>,
    pub interrupted: bool,
    pub engine_aborted: bool,
    pub named: Vec<WatchNamed>,
    pub lang_passed: [usize; 2],
    pub lang_failed: [usize; 2],
    pub lang_timed_out: [usize; 2],
}

#[cfg(test)]
fn cli_report_repo() -> Option<std::path::PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    crate::test_git::git_repo_root(&cwd).ok()
}

pub(crate) fn kiss_report_from_ensure_outcome(
    outcome: crate::test_runner::target_request::EnsureOutcome,
) -> KissTestReport {
    match outcome {
        crate::test_runner::target_request::EnsureOutcome::Ready { report, replay } => {
            let hit = kiss_report_from_target(&report).unwrap_or_default();
            if replay {
                suite_report::replay_suite_report(&hit);
            }
            hit
        }
        crate::test_runner::target_request::EnsureOutcome::Interrupted { ready } => match ready {
            Some(report) => {
                let mut hit = kiss_report_from_target(&report).unwrap_or_default();
                hit.interrupted = true;
                hit.exit_code = EXIT_INTERRUPTED;
                hit
            }
            None => interrupted_empty(),
        },
        crate::test_runner::target_request::EnsureOutcome::Miss {
            exit_code,
            error,
            engine_aborted,
            typed,
        } => {
            if typed {
                typed_without_transcript(exit_code, error, engine_aborted)
            } else {
                finish_report(exit_code, error, engine_aborted)
            }
        }
        crate::test_runner::target_request::EnsureOutcome::Moved { exit_code } => {
            let taken = kiss::watch_report::take_watch_report_taken().unwrap_or_default();
            let mut report = report_from_taken(exit_code, None, false, taken);
            let (text, _) = result_text(&report.lines);
            report.output = (!text.is_empty()).then_some(text);
            report
        }
    }
}

#[cfg(test)]
pub(crate) fn run_kiss_test_report<F>(args: RunTestCmdArgs<'_>, run_tests: F) -> KissTestReport
where
    F: FnMut(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
{
    let owned = cli_report_repo();
    run_kiss_test_report_reuse(args, run_tests, true, owned.as_deref())
}

#[cfg(test)]
pub(crate) fn run_kiss_test_report_reuse<F>(
    args: RunTestCmdArgs<'_>,
    run_tests: F,
    reuse: bool,
    repo_root: Option<&std::path::Path>,
) -> KissTestReport
where
    F: FnMut(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
{
    kiss_report_from_ensure_outcome(crate::test_runner::target_request::ensure_target_report(
        repo_root,
        &args,
        crate::test_runner::target_request::EnsureChoice {
            reuse_ready: reuse,
            close_zero: false,
        },
        run_tests,
    ))
}

pub(crate) fn repo_can_assemble_reports(repo_root: Option<&std::path::Path>) -> bool {
    repo_root.is_some_and(|repo| repo != std::path::Path::new(".") && repo.join(".git").exists())
}

fn interrupted_empty() -> KissTestReport {
    let _ = kiss::watch_report::take_watch_report_taken();
    KissTestReport {
        exit_code: EXIT_INTERRUPTED,
        interrupted: true,
        ..KissTestReport::default()
    }
}

fn typed_without_transcript(
    exit_code: i32,
    error: Option<String>,
    engine_aborted: bool,
) -> KissTestReport {
    let taken = kiss::watch_report::take_watch_report_taken().unwrap_or_default();
    let mut report = report_from_taken(exit_code, error, false, taken);
    report.engine_aborted = engine_aborted;
    if let Some(output) = cached_result_text(&report.lines) {
        report.output = Some(output);
        if report.exit_code == 124 {
            report.exit_code = 1;
        }
        report.error = None;
    } else {
        report.output = None;
    }
    report
}

fn cached_result_text(lines: &[String]) -> Option<String> {
    let (out, has_bad) = result_text(lines);
    has_bad.then_some(out)
}

fn result_text(lines: &[String]) -> (String, bool) {
    let mut out = String::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut has_bad = false;
    for line in lines {
        let trimmed = line.trim();
        if let Some(row) = status_row(trimmed, "FAIL").or_else(|| status_row(trimmed, "TIMEOUT")) {
            has_bad = true;
            if seen.insert(row.clone()) {
                out.push_str(&row);
                out.push('\n');
            }
        } else if trimmed.starts_with('✓') || trimmed.starts_with('✗') {
            let summary = trimmed.split(" · ").take(3).collect::<Vec<_>>().join(" · ");
            if seen.insert(summary.clone()) {
                out.push_str(&summary);
                out.push('\n');
            }
        }
    }
    (out, has_bad)
}

fn status_row(line: &str, label: &str) -> Option<String> {
    let rest = line
        .strip_prefix(&format!("{label}:"))
        .or_else(|| line.strip_prefix(&format!("{label} ")))?;
    let selector = rest.trim().split(" (").next().unwrap_or("").trim();
    if selector.is_empty() {
        return None;
    }
    Some(format!("{label} {selector}"))
}

fn finish_report(exit_code: i32, error: Option<String>, engine_aborted: bool) -> KissTestReport {
    let mut report = report_from_taken(
        exit_code,
        error,
        false,
        kiss::watch_report::take_watch_report_taken().unwrap_or_default(),
    );
    report.engine_aborted = engine_aborted;
    report
}

fn report_from_taken(
    exit_code: i32,
    error: Option<String>,
    interrupted: bool,
    taken: kiss::watch_report::WatchReportTaken,
) -> KissTestReport {
    let output = kiss::watch_report::transcript_from_lines(&taken.lines);
    KissTestReport {
        exit_code,
        output,
        lines: taken.lines,
        totals: taken.totals,
        error,
        interrupted,
        engine_aborted: false,
        named: taken.named,
        lang_passed: taken.lang_passed,
        lang_failed: taken.lang_failed,
        lang_timed_out: taken.lang_timed_out,
    }
}

pub(crate) fn clone_run_args<'a>(args: &RunTestCmdArgs<'a>) -> RunTestCmdArgs<'a> {
    let request = request_from_run_args(args);
    RunTestCmdArgs {
        doubles: args.doubles.clone(),
        invocation: to_compat_invocation(&request),
        target_request: request,
        main_branch_cli: args.main_branch_cli,
        base_branch_cli: args.base_branch_cli,
        dry_run: args.dry_run,
        force_rerun: args.force_rerun,
        force_bad: args.force_bad,
        metrics: args.metrics,
        jobs: args.jobs,
        extras: args.extras,
        config_main_branch: args.config_main_branch,
        gate_config: args.gate_config.clone(),
    }
}

#[cfg(test)]
#[path = "kiss_test_report_test.rs"]
mod tests;
