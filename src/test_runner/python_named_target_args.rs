//! Shared RunTestCmdArgs builders for explicit Python target unit tests.

use crate::bin_cli::args::TestInvocation;
use crate::test_runner::RunTestCmdArgs;

pub(crate) fn python_named_target_args(
    target: &str,
    force_rerun: bool,
) -> RunTestCmdArgs<'static> {
    RunTestCmdArgs {
        invocation: TestInvocation::Targets(vec![target.to_string()]),
        main_branch_cli: None,
        base_branch_cli: None,
        dry_run: false,
        force_rerun,
        force_bad: false,
        metrics: false,
        jobs: 1,
        extra: &[],
        python_extra: &[],
        ignore: &[],
        lang_filter: Some(kiss::Language::Python),
        config_main_branch: None,
    gate_config: kiss::GateConfig::default()
    }
}
