//! Shared git + synthetic-coverage fixtures for change-mode planning tests.

mod git;
mod python_warm;
mod rust_warm;

pub(crate) use git::{
    checkout_branch, ensure_main_branch, git_in, git_stdout, init_git, with_cwd,
};
pub(crate) use python_warm::{
    PY_COVERING_SELECTOR, edit_python_covered_source, rewrite_python_population_after_edit,
    warm_python_covering_demo,
};
pub(crate) use rust_warm::{
    RS_COVERING_SELECTOR, assert_base_delta_plan, edit_rust_covered_source,
    warm_base_demo_with_historical_source, warm_committed_rust_demo, warm_multi_branch_rust_demo,
};
