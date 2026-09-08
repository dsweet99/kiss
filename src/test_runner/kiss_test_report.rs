use super::{RunTestCmdArgs, RunTestOnceOutcome, WatchCoverageResult};

pub(crate) const EXIT_INTERRUPTED: i32 = 130;

pub(crate) const KISS_TEST_ALLOW_REFRESH: bool = false;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KissTestReport {
    pub exit_code: i32,
    pub output: Option<String>,
    pub lines: Vec<String>,
    pub error: Option<String>,
    pub interrupted: bool,
}

pub(crate) fn run_kiss_test_report<F, C>(
    args: RunTestCmdArgs<'_>,
    mut run_tests: F,
    mut run_cov: C,
) -> KissTestReport
where
    F: FnMut(RunTestCmdArgs<'_>) -> RunTestOnceOutcome,
    C: FnMut(&RunTestCmdArgs<'_>) -> WatchCoverageResult,
{
    kiss::rust_llvm_cov_runner::begin_watch_report_capture();
    let test_exit = match run_tests(clone_run_args(&args)) {
        RunTestOnceOutcome::Interrupted => return interrupted_report(),
        RunTestOnceOutcome::Code(code) => code,
    };
    if test_exit != 0 || args.dry_run {
        return finish_report(test_exit, None);
    }
    let cov = run_cov(&args);
    if cov.interrupted {
        return interrupted_report();
    }
    finish_report(cov.exit_code, cov.error)
}

fn interrupted_report() -> KissTestReport {
    let (lines, output) = take_transcript();
    KissTestReport {
        exit_code: EXIT_INTERRUPTED,
        output,
        lines,
        error: None,
        interrupted: true,
    }
}

fn finish_report(exit_code: i32, error: Option<String>) -> KissTestReport {
    let (lines, output) = take_transcript();
    KissTestReport {
        exit_code,
        output,
        lines,
        error,
        interrupted: false,
    }
}

fn take_transcript() -> (Vec<String>, Option<String>) {
    let lines = kiss::rust_llvm_cov_runner::take_watch_report_lines().unwrap_or_default();
    let output = kiss::rust_llvm_cov_runner::transcript_from_lines(&lines);
    (lines, output)
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
