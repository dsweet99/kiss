use crate::bin_cli::test_cmd::TestCommandArgs;
use crate::test_runner::target_request::TargetRequest;
use crate::test_runner::{RunTestCmdArgs, WatchCoverageParams, WatchCoverageResult};

fn coverage_from_ready_request(request: &TargetRequest, coverage_all: bool) -> i32 {
    if let Some(exit) =
        crate::test_runner::target_request::coverage_exit_from_ready_request(request, coverage_all)
    {
        return exit;
    }
    eprintln!("error: kiss test: incomplete coverage evidence");
    1
}

pub(crate) fn evaluate_watch_coverage(
    cycle: &RunTestCmdArgs<'_>,
    cov: &WatchCoverageParams<'_>,
) -> WatchCoverageResult {
    coverage_result_from_exit(coverage_from_ready_request(
        &crate::test_runner::target_request::request_from_run_args(cycle),
        cov.coverage_all,
    ))
}

#[allow(dead_code)]
pub(super) fn coverage_after_kiss_test(args: &TestCommandArgs<'_>) -> WatchCoverageResult {
    coverage_result_from_exit(coverage_from_ready_request(
        &request_from_test_args(args),
        args.coverage_all,
    ))
}

pub(crate) fn coverage_result_from_exit(cov_code: i32) -> WatchCoverageResult {
    if crate::test_runner::consume_rust_batch_interrupted() {
        return WatchCoverageResult::interrupted();
    }
    if cov_code == 0 {
        WatchCoverageResult::ok(0)
    } else {
        WatchCoverageResult::failed(cov_code, "coverage gate failed")
    }
}

#[cfg(test)]
pub(crate) fn finish_with_coverage(args: &TestCommandArgs<'_>, test_exit: i32) -> i32 {
    let cov_code = coverage_from_ready_request(&request_from_test_args(args), args.coverage_all);
    if test_exit != 0 { test_exit } else { cov_code }
}

pub(crate) fn request_from_test_args(args: &TestCommandArgs<'_>) -> TargetRequest {
    crate::test_runner::target_request::request_from_invocation(
        &args.invocation,
        args.main_branch,
        args.base_branch,
        args.test_cfg.main_branch.as_deref(),
        args.lang_filter,
        args.ignore,
    )
}
