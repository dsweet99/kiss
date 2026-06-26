use kiss::TestSectionConfig;

use crate::test_git::TestChangeMode;
use crate::test_runner::{RunTestCmdArgs, ValidateSelectionCmdArgs, run_test, validate_selection};

pub struct TestCommandArgs<'a> {
    pub mode: TestChangeMode,
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

pub struct ValidateSelectionCommandArgs<'a> {
    pub mode: TestChangeMode,
    pub main_branch: Option<&'a str>,
    pub base_branch: Option<&'a str>,
    pub dry_run: bool,
    pub jobs: usize,
    pub ignore: &'a [String],
    pub extra: &'a [String],
    pub lang_filter: Option<kiss::Language>,
    pub fixture: Option<&'a str>,
    pub test_cfg: &'a TestSectionConfig,
}

pub fn run_test_command(args: TestCommandArgs<'_>) -> i32 {
    run_test(RunTestCmdArgs {
        mode: args.mode,
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

pub fn run_validate_selection_command(args: ValidateSelectionCommandArgs<'_>) -> i32 {
    validate_selection(ValidateSelectionCmdArgs {
        mode: args.mode,
        main_branch_cli: args.main_branch,
        base_branch_cli: args.base_branch,
        dry_run: args.dry_run,
        jobs: args.jobs,
        extra: args.extra,
        ignore: args.ignore,
        lang_filter: args.lang_filter,
        fixture: args.fixture,
        config_main_branch: args.test_cfg.main_branch.as_deref(),
    })
}
