use kiss::TestSectionConfig;

use crate::bin_cli::args::TestInvocation;
use crate::test_runner::{RunTestCmdArgs, run_test};

pub struct TestCommandArgs<'a> {
    pub invocation: TestInvocation,
    pub main_branch: Option<&'a str>,
    pub base_branch: Option<&'a str>,
    pub dry_run: bool,
    pub force: bool,
    pub metrics: bool,
    pub jobs: usize,
    pub ignore: &'a [String],
    pub extra: &'a [String],
    pub lang_filter: Option<kiss::Language>,
    pub test_cfg: &'a TestSectionConfig,
}

pub fn run_test_command(args: TestCommandArgs<'_>) -> i32 {
    run_test(RunTestCmdArgs {
        invocation: args.invocation,
        main_branch_cli: args.main_branch,
        base_branch_cli: args.base_branch,
        dry_run: args.dry_run,
        force_rerun: args.force,
        metrics: args.metrics,
        jobs: args.jobs,
        extra: args.extra,
        ignore: args.ignore,
        lang_filter: args.lang_filter,
        config_main_branch: args.test_cfg.main_branch.as_deref(),
    })
}
