pub(crate) mod check_line_coverage;
pub(crate) mod check_runtime_refresh;
mod coverage_decision;
pub(crate) mod duration;
pub(crate) mod last_status;
mod line_selection;
mod python_cache_path;
mod python_coverage_index;
mod run_logic;
mod runners;
mod rust_coverage_index;
mod rust_llvm_cov;
mod targets;

use std::path::PathBuf;
use std::time::Duration;

use kiss::Language;

use crate::bin_cli::args::TestInvocation;
use crate::test_git::TestChangeMode;
pub(crate) use run_logic::run_selectors;

#[cfg(test)]
pub(crate) struct TestEnvVarGuard {
    key: &'static str,
    old: Option<String>,
}

#[cfg(test)]
impl TestEnvVarGuard {
    pub(crate) fn set(key: &'static str, value: &str) -> Self {
        let old = std::env::var(key).ok();
        unsafe { std::env::set_var(key, value) };
        Self { key, old }
    }
}

#[cfg(test)]
impl Drop for TestEnvVarGuard {
    fn drop(&mut self) {
        match &self.old {
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

pub struct RunTestCmdArgs<'a> {
    pub invocation: TestInvocation,
    pub main_branch_cli: Option<&'a str>,
    pub base_branch_cli: Option<&'a str>,
    pub dry_run: bool,
    pub force_rerun: bool,
    pub metrics: bool,
    pub jobs: usize,
    pub extra: &'a [String],
    pub ignore: &'a [String],
    pub lang_filter: Option<Language>,
    pub config_main_branch: Option<&'a str>,
}

pub fn run_test(a: RunTestCmdArgs<'_>) -> i32 {
    let dry_run = a.dry_run;
    let force_rerun = a.force_rerun;
    let metrics = a.metrics;
    let jobs = a.jobs;
    let extra = a.extra;
    let plan_started = std::time::Instant::now();
    match plan_for_invocation(&a) {
        Ok(mut planned) => {
            apply_cold_initialization_population(&a, &mut planned);
            match run_selectors(
                &planned,
                SelectorRunOptions {
                    dry_run,
                    force_rerun,
                    metrics,
                    jobs,
                    extra,
                    plan_duration: plan_started.elapsed(),
                },
            ) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            }
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn plan_for_invocation(a: &RunTestCmdArgs<'_>) -> Result<PlannedSelectors, String> {
    match &a.invocation {
        TestInvocation::Commit => plan_selectors(
            TestChangeMode::Commit,
            a.main_branch_cli,
            a.base_branch_cli,
            a.ignore,
            a.extra,
            a.lang_filter,
            a.config_main_branch,
        ),
        TestInvocation::Base => plan_selectors(
            TestChangeMode::Base,
            a.main_branch_cli,
            a.base_branch_cli,
            a.ignore,
            a.extra,
            a.lang_filter,
            a.config_main_branch,
        ),
        TestInvocation::Main => plan_selectors(
            TestChangeMode::Main,
            a.main_branch_cli,
            a.base_branch_cli,
            a.ignore,
            a.extra,
            a.lang_filter,
            a.config_main_branch,
        ),
        TestInvocation::All => plan_target_selectors(
            TargetPlanKind::All,
            a.ignore,
            a.extra,
            a.lang_filter,
        ),
        TestInvocation::Targets(targets) => plan_target_selectors(
            TargetPlanKind::Targets(targets.as_slice()),
            a.ignore,
            a.extra,
            a.lang_filter,
        ),
    }
}

pub(crate) fn should_force_cold_initialization(a: &RunTestCmdArgs<'_>, repo_root: &std::path::Path) -> bool {
    matches!(
        a.invocation,
        TestInvocation::Base | TestInvocation::Main
    ) && !a.dry_run
        && !a.force_rerun
        && !a.metrics
        && a.extra.is_empty()
        && a.ignore.is_empty()
        && a.lang_filter.is_none()
        && !repo_root.join(".kiss").exists()
}

pub(crate) fn apply_cold_initialization_population(a: &RunTestCmdArgs<'_>, planned: &mut PlannedSelectors) {
    if !should_force_cold_initialization(a, &planned.repo_root) {
        return;
    }
    match a.lang_filter {
        Some(Language::Python) => planned.python_population_required = true,
        Some(Language::Rust) => planned.rust_population_required = true,
        None => {
            planned.python_population_required = true;
            planned.rust_population_required = true;
        }
    }
}

pub(crate) struct PlannedSelectors {
    pub repo_root: PathBuf,
    pub py_sel: Vec<String>,
    pub rs_sel: Vec<String>,
    pub python_population_required: bool,
    pub rust_population_required: bool,
    pub rust_source_paths: Vec<PathBuf>,
    pub rust_vcs_source_paths: usize,
    pub rust_snapshot_delta_modified: usize,
    pub rust_snapshot_delta_structural: bool,
    pub python_prior_failure_selectors: Vec<String>,
    pub rust_prior_failure_selectors: Vec<String>,
    pub coverage_decision_engine_used: bool,
    pub rust_selection_basis: crate::test_runner::coverage_decision::RustSelectionBasis,
    pub ignore: Vec<String>,
}

pub(crate) struct SelectorRunOptions<'a> {
    pub dry_run: bool,
    pub force_rerun: bool,
    pub metrics: bool,
    pub jobs: usize,
    pub extra: &'a [String],
    pub plan_duration: Duration,
}

mod plan;
pub(crate) use plan::{TargetPlanKind, plan_selectors, plan_target_selectors};

#[cfg(test)]
pub(crate) mod test_mode_fixtures;

#[cfg(test)]
#[path = "test_change_modes_test.rs"]
mod test_change_modes_test;

#[cfg(test)]
#[path = "test_change_modes_b_test.rs"]
mod test_change_modes_b_test;

#[cfg(test)]
#[path = "mod_test.rs"]
mod mod_test;

#[cfg(test)]
#[path = "mod_run_api_test.rs"]
mod mod_run_api_test;

#[cfg(test)]
#[path = "python_coverage_index_witness_test.rs"]
mod python_coverage_index_witness_test;

#[cfg(test)]
#[path = "runners_reusable_prior_cli_acceptance_test.rs"]
mod runners_reusable_prior_cli_acceptance_test;
#[cfg(test)]
#[path = "runners_reusable_prior_compile_time_test.rs"]
mod runners_reusable_prior_compile_time_test;
#[cfg(test)]
#[path = "runners_reusable_prior_test.rs"]
mod runners_reusable_prior_test;
#[cfg(test)]
#[path = "runners_test.rs"]
mod runners_test;

#[cfg(test)]
#[path = "runners_workspace_test.rs"]
mod runners_workspace_test;

#[cfg(test)]
#[path = "runners_request_test.rs"]
mod runners_request_test;

#[cfg(test)]
#[path = "rust_batch_witness_test.rs"]
mod rust_batch_witness_test;

#[cfg(test)]
#[path = "rust_batch_witness_derived_test.rs"]
mod rust_batch_witness_derived_test;

#[cfg(test)]
#[path = "test_cli_acceptance_test.rs"]
mod test_cli_acceptance_test;
