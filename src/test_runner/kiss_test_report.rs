use kiss::rust_llvm_cov_runner::{WatchNamed, WatchNamedOutcome, WatchSuiteTotals};

use super::RunTestCmdArgs;
#[cfg(test)]
use super::{RunTestOnceOutcome, WatchCoverageResult};
use crate::test_runner::target_request::{
    EffectiveStatus, TargetReport, request_from_run_args, to_compat_invocation,
};

#[allow(dead_code)]
#[path = "suite_report.rs"]
mod suite_report;

pub(crate) const EXIT_INTERRUPTED: i32 = 130;

#[allow(dead_code)]
pub(crate) const KISS_TEST_ALLOW_REFRESH: bool = false;

fn query_ensure_policy(coverage_all: bool) -> crate::test_runner::target_request::EnsurePolicy {
    crate::test_runner::target_request::EnsurePolicy {
        dry_run: false,
        require_complete: true,
        inject_mismatch: false,
        retry_bad: false,
        coverage_all,
        assemble_only: false,
    }
}

pub(crate) fn kiss_report_from_ensure_query(
    args: &RunTestCmdArgs<'_>,
    repo_root: Option<&std::path::Path>,
) -> Option<KissTestReport> {
    if args.dry_run || args.force_rerun || args.force_bad {
        return None;
    }
    let repo = repo_root?;
    match crate::test_runner::target_request::ensure_target_report_query(
        repo,
        &request_from_run_args(args),
        &query_ensure_policy(args.coverage_all),
        args.extra,
    ) {
        Ok(crate::test_runner::target_request::Ensured::Report(report)) => {
            kiss_report_from_target(&report)
        }
        Err(_) => None,
    }
}

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
        let language = match row.language.as_str() {
            "python" => kiss::Language::Python,
            "rust" => kiss::Language::Rust,
            _ => continue,
        };
        let i = match language {
            kiss::Language::Python => 0,
            kiss::Language::Rust => 1,
        };
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
    }
}

#[cfg(test)]
pub(crate) fn run_kiss_test_report<F, C>(
    args: RunTestCmdArgs<'_>,
    run_tests: F,
    run_cov: C,
) -> KissTestReport
where
    F: FnMut(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
    C: FnMut(&RunTestCmdArgs<'_>) -> WatchCoverageResult,
{
    let owned = cli_report_repo();
    run_kiss_test_report_reuse(args, run_tests, run_cov, true, owned.as_deref())
}

#[cfg(test)]
pub(crate) fn run_kiss_test_report_reuse<F, C>(
    args: RunTestCmdArgs<'_>,
    run_tests: F,
    run_cov: C,
    reuse: bool,
    repo_root: Option<&std::path::Path>,
) -> KissTestReport
where
    F: FnMut(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
    C: FnMut(&RunTestCmdArgs<'_>) -> WatchCoverageResult,
{
    let _ = run_cov;
    kiss_report_from_ensure_outcome(crate::test_runner::target_request::ensure_target_report(
        repo_root, &args, reuse, false, run_tests,
    ))
}

pub(crate) fn repo_can_assemble_reports(repo_root: Option<&std::path::Path>) -> bool {
    repo_root.is_some_and(|repo| repo != std::path::Path::new(".") && repo.join(".git").exists())
}

fn interrupted_empty() -> KissTestReport {
    let _ = kiss::rust_llvm_cov_runner::take_watch_report_taken();
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
    let _ = kiss::rust_llvm_cov_runner::take_watch_report_taken();
    KissTestReport {
        exit_code,
        error,
        engine_aborted,
        ..KissTestReport::default()
    }
}

fn finish_report(exit_code: i32, error: Option<String>, engine_aborted: bool) -> KissTestReport {
    let mut report = report_from_taken(
        exit_code,
        error,
        false,
        kiss::rust_llvm_cov_runner::take_watch_report_taken().unwrap_or_default(),
    );
    report.engine_aborted = engine_aborted;
    report
}

fn report_from_taken(
    exit_code: i32,
    error: Option<String>,
    interrupted: bool,
    taken: kiss::rust_llvm_cov_runner::WatchReportTaken,
) -> KissTestReport {
    let output = kiss::rust_llvm_cov_runner::transcript_from_lines(&taken.lines);
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
        invocation: to_compat_invocation(&request),
        target_request: request,
        main_branch_cli: args.main_branch_cli,
        base_branch_cli: args.base_branch_cli,
        dry_run: args.dry_run,
        force_rerun: args.force_rerun,
        force_bad: args.force_bad,
        metrics: args.metrics,
        coverage_all: args.coverage_all,
        jobs: args.jobs,
        extra: args.extra,
        python_extra: args.python_extra,
        ignore: args.ignore,
        lang_filter: args.lang_filter,
        config_main_branch: args.config_main_branch,
        gate_config: args.gate_config.clone(),
    }
}

#[cfg(test)]
#[path = "kiss_test_report_test.rs"]
mod tests;
