use kiss::TestSectionConfig;

use crate::test_git::TestChangeMode;
use crate::test_runner::{run_test, RunTestCmdArgs};

#[allow(clippy::too_many_arguments)]
pub fn run_test_command(
    mode: TestChangeMode,
    main_branch: Option<&str>,
    base_branch: Option<&str>,
    dry_run: bool,
    ignore: &[String],
    extra: &[String],
    lang_filter: Option<kiss::Language>,
    test_cfg: &TestSectionConfig,
) -> i32 {
    run_test(RunTestCmdArgs {
        mode,
        main_branch_cli: main_branch,
        base_branch_cli: base_branch,
        dry_run,
        extra,
        ignore,
        lang_filter,
        config_main_branch: test_cfg.main_branch.as_deref(),
    })
}
