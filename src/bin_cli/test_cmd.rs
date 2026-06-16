use kiss::TestSectionConfig;

use crate::test_git::TestChangeMode;
use crate::test_runner::{RunTestCmdArgs, run_test};

pub struct TestCommandArgs<'a> {
    pub mode: TestChangeMode,
    pub main_branch: Option<&'a str>,
    pub base_branch: Option<&'a str>,
    pub dry_run: bool,
    pub ignore: &'a [String],
    pub extra: &'a [String],
    pub lang_filter: Option<kiss::Language>,
    pub jobs: Option<usize>,
    pub test_cfg: &'a TestSectionConfig,
}

pub fn run_test_command(args: TestCommandArgs<'_>) -> i32 {
    run_test(RunTestCmdArgs {
        mode: args.mode,
        main_branch_cli: args.main_branch,
        base_branch_cli: args.base_branch,
        dry_run: args.dry_run,
        extra: args.extra,
        ignore: args.ignore,
        lang_filter: args.lang_filter,
        jobs: args.jobs,
        config_main_branch: args.test_cfg.main_branch.as_deref(),
    })
}
