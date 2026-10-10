use crate::bin_cli::args::TestInvocation;
use crate::test_runner::RunTestCmdArgs;
use kiss::Language;

pub(crate) fn dry_run_cmd_args<'a>(
    invocation: TestInvocation,
    ignore: &'a [String],
    jobs: usize,
    lang_filter: Option<Language>,
) -> RunTestCmdArgs<'a> {
    let request = crate::test_runner::target_request::request_from_invocation(
        &invocation,
        None,
        None,
        None,
        lang_filter,
        ignore,
    );
    RunTestCmdArgs {
        doubles: None,
        invocation: crate::test_runner::target_request::to_compat_invocation(&request),
        target_request: request,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        force_rerun: false,
        metrics: false,
        jobs,
        extras: crate::test_runner::language_keyed::LanguageKeyed::EMPTY,
        config_main_branch: None,
        gate_config: kiss::GateConfig::default(),
    }
}
