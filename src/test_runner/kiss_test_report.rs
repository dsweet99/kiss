use kiss::rust_llvm_cov_runner::{WatchNamed, WatchSuiteTotals};

use super::{RunTestCmdArgs, RunTestOnceOutcome, WatchCoverageResult};

#[path = "suite_report.rs"]
mod suite_report;

pub(crate) const EXIT_INTERRUPTED: i32 = 130;

pub(crate) const KISS_TEST_ALLOW_REFRESH: bool = false;

pub(crate) fn durable_lang_reply(
    repo: &std::path::Path,
    lang: kiss::Language,
    ignore: &[String],
    extra: &[String],
    python_extra: &[String],
) -> Option<(i32, String)> {
    suite_report::durable_lang_reply(repo, lang, ignore, extra, python_extra)
}

pub(crate) fn durable_all_reply(
    repo: &std::path::Path,
    ignore: &[String],
    extra: &[String],
    python_extra: &[String],
) -> Option<(i32, String)> {
    suite_report::durable_all_reply(repo, ignore, extra, python_extra)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct KissTestReport {
    pub exit_code: i32,
    pub output: Option<String>,
    pub lines: Vec<String>,
    pub totals: Option<WatchSuiteTotals>,
    pub error: Option<String>,
    pub interrupted: bool,
    pub named: Vec<WatchNamed>,
    pub lang_passed: [usize; 2],
    pub lang_failed: [usize; 2],
    pub lang_timed_out: [usize; 2],
}

pub(crate) fn run_kiss_test_report<F, C>(
    args: RunTestCmdArgs<'_>,
    run_tests: F,
    run_cov: C,
) -> KissTestReport
where
    F: FnMut(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
    C: FnMut(&RunTestCmdArgs<'_>) -> WatchCoverageResult,
{
    run_kiss_test_report_reuse(args, run_tests, run_cov, true, None)
}

pub(crate) fn run_kiss_test_report_reuse<F, C>(
    args: RunTestCmdArgs<'_>,
    mut run_tests: F,
    mut run_cov: C,
    reuse: bool,
    repo_root: Option<&std::path::Path>,
) -> KissTestReport
where
    F: FnMut(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
    C: FnMut(&RunTestCmdArgs<'_>) -> WatchCoverageResult,
{
    if reuse && let Some(hit) = suite_report::load_fresh_suite_report(&args, repo_root) {
        suite_report::replay_suite_report(&hit);
        return hit;
    }
    kiss::rust_llvm_cov_runner::begin_watch_report_capture();
    let (exit_code, error, interrupted) = {
        let _defer = crate::test_runner::final_summary::RecapDeferGuard::enter();
        match run_tests(clone_run_args(&args)) {
            RunTestOnceOutcome::Interrupted => (EXIT_INTERRUPTED, None, true),
            RunTestOnceOutcome::Code(code) if code != 0 || args.dry_run => (code, None, false),
            RunTestOnceOutcome::Code(_) => {
                let cov = run_cov(&args);
                if cov.interrupted {
                    (EXIT_INTERRUPTED, None, true)
                } else {
                    (cov.exit_code, cov.error, false)
                }
            }
        }
    };
    if interrupted {
        interrupted_report()
    } else {
        let report = finish_report(exit_code, error);
        suite_report::persist_suite_report(&args, &report, repo_root);
        report
    }
}

fn interrupted_report() -> KissTestReport {
    report_from_taken(
        EXIT_INTERRUPTED,
        None,
        true,
        kiss::rust_llvm_cov_runner::take_watch_report_taken().unwrap_or_default(),
    )
}

fn finish_report(exit_code: i32, error: Option<String>) -> KissTestReport {
    report_from_taken(
        exit_code,
        error,
        false,
        kiss::rust_llvm_cov_runner::take_watch_report_taken().unwrap_or_default(),
    )
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
        named: taken.named,
        lang_passed: taken.lang_passed,
        lang_failed: taken.lang_failed,
        lang_timed_out: taken.lang_timed_out,
    }
}

pub(crate) fn clone_run_args<'a>(args: &RunTestCmdArgs<'a>) -> RunTestCmdArgs<'a> {
    RunTestCmdArgs {
        invocation: args.invocation.clone(),
        main_branch_cli: args.main_branch_cli,
        base_branch_cli: args.base_branch_cli,
        dry_run: args.dry_run,
        force_rerun: args.force_rerun,
        force_bad: args.force_bad,
        metrics: args.metrics,
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
