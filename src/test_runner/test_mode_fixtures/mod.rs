mod git;
mod python_warm;
mod rust_warm;
mod run_args;

pub(crate) use git::{checkout_branch, ensure_main_branch, git_in, git_stdout, init_git, with_cwd};
pub(crate) use python_warm::{
    PY_COVERING_SELECTOR, edit_python_covered_source, rewrite_python_population_after_edit,
    warm_python_covering_demo, with_locked_warm_python_repo,
};
pub(crate) use rust_warm::{
    RS_COVERING_SELECTOR, assert_base_delta_plan, clone_row_b_committed_repo,
    clone_warm_committed_repo, clone_warm_demo_repo, edit_rust_covered_source,
    seeded_population_matches_current_context, warm_committed_rust_demo,
    with_locked_base_historical_repo, with_locked_warm_committed_repo, with_locked_warm_demo_repo,
};
pub(crate) use run_args::{dry_run_cmd_args, python_dry_run_args};

mod planned;
pub(crate) use planned::empty_planned_selectors;
