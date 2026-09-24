use crate::bin_cli::args::TestInvocation;
use crate::test_runner::RunTestCmdArgs;
use crate::test_runner::target_request::{operands_request, to_compat_invocation};

pub(crate) fn python_named_target_args(target: &str, force_rerun: bool) -> RunTestCmdArgs<'static> {
    let request = operands_request(&[target.to_string()], Some(kiss::Language::Python), &[]);
    RunTestCmdArgs {
        invocation: to_compat_invocation(&request),
        target_request: request,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: false,
        force_rerun,
        force_bad: false,
        metrics: false,
        coverage_all: false,
        jobs: 1,
        extra: &[],
        python_extra: &[],
        ignore: &[],
        lang_filter: Some(kiss::Language::Python),
        config_main_branch: None,
        gate_config: kiss::GateConfig::default(),
    }
}

#[test]
fn python_named_target_args_uses_operands_request() {
    let args = python_named_target_args("tests/b.py::test_z", false);
    let request = &args.target_request;
    assert_eq!(
        request,
        &operands_request(
            &["tests/b.py::test_z".into()],
            Some(kiss::Language::Python),
            &[],
        )
    );
    assert_eq!(args.invocation, to_compat_invocation(request));
    assert_eq!(
        args.invocation,
        TestInvocation::Targets(vec!["tests/b.py::test_z".into()])
    );
}
