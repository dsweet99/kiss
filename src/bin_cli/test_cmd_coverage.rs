use crate::bin_cli::test_cmd::TestCommandArgs;
use crate::test_runner::target_request::TargetRequest;

#[cfg(test)]
fn coverage_from_ready_request(
    request: &TargetRequest,
    coverage_all: bool,
    extras: crate::test_runner::language_keyed::LanguageKeyed<&[String]>,
) -> i32 {
    if let Some(exit) = crate::test_runner::target_request::coverage_exit_from_ready_request(
        request,
        coverage_all,
        extras,
    ) {
        return exit;
    }
    eprintln!("error: kiss test: incomplete coverage evidence");
    1
}

#[cfg(test)]
pub(crate) fn finish_with_coverage(args: &TestCommandArgs<'_>, test_exit: i32) -> i32 {
    let python_extra_owned =
        kiss::effective_python_pytest_args(&args.test_cfg.pytest_plugins, args.extra);
    let extras = crate::test_runner::language_keyed::LanguageKeyed {
        rust: args.extra,
        python: python_extra_owned.as_slice(),
    };
    let cov_code =
        coverage_from_ready_request(&request_from_test_args(args), args.coverage_all, extras);
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
