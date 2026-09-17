use crate::bin_cli::args::TestInvocation;
use crate::test_runner::RunTestCmdArgs;
use kiss::Language;

pub(crate) fn python_dry_run_args(targets: Vec<String>) -> RunTestCmdArgs<'static> {
    dry_run_cmd_args(TestInvocation::Targets(targets), &[], 1, Some(Language::Python))
}

pub(crate) fn dry_run_cmd_args<'a>(
    invocation: TestInvocation,
    ignore: &'a [String],
    jobs: usize,
    lang_filter: Option<Language>,
) -> RunTestCmdArgs<'a> {
    RunTestCmdArgs {
        invocation,
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: true,
        force_rerun: false,
        force_bad: false,
        metrics: false,
        jobs,
        extra: &[],
        python_extra: &[],
        ignore,
        lang_filter,
        config_main_branch: None,
        gate_config: kiss::GateConfig::default(),
    }
}
