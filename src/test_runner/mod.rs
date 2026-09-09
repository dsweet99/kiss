#[cfg(test)]
#[path = "capture_stdout.rs"]
pub(crate) mod capture_stdout;

pub(crate) mod check_line_coverage;
pub(crate) mod check_runtime_refresh;
mod coverage_decision;
pub(crate) mod coverage_index;
pub(crate) mod duration;
pub(crate) mod ensure_runtime;
mod execution_generation;
pub(crate) mod execution_witness;
pub(crate) mod force_bad;
pub(crate) use force_bad::apply_force_bad;
mod final_summary;
pub(crate) mod lang_iface;
pub(crate) mod lang_python;
pub(crate) mod lang_rust;
mod language_keyed;
pub(crate) mod last_status;
mod line_selection;
mod planned_selectors;
mod python_cache_path;
pub(crate) mod python_coverage_index;
mod run_logic;
mod runners;
mod rust_batch_interrupt;
mod rust_coverage_index;
mod rust_report_id_cache;
mod selector_ids;
mod status_labels;
mod targets;
pub(crate) use targets::expand_target_operands;
pub(crate) mod tests_remaining;
pub(crate) mod unit_test_timing;
mod kiss_test_report;
mod watch;
#[cfg(test)]
pub(crate) use planned_selectors::should_force_cold_initialization;
pub(crate) use planned_selectors::{PlannedSelectors, SelectorRunOptions, empty_planned};
#[cfg(test)]
pub(crate) use planned_selectors::{
    apply_cold_initialization_population, apply_force_all_population,
};
pub(crate) use rust_batch_interrupt::consume_rust_batch_interrupted;
pub(crate) use kiss_test_report::{clone_run_args, run_kiss_test_report, KISS_TEST_ALLOW_REFRESH};

pub(crate) use lang_rust::llvm_cov as rust_llvm_cov;

use kiss::Language;

use crate::bin_cli::args::TestInvocation;
#[cfg(test)]
use crate::test_git::TestChangeMode;
#[cfg(test)]
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
    pub force_bad: bool,
    pub metrics: bool,
    pub jobs: usize,
    pub extra: &'a [String],
    pub python_extra: &'a [String],
    pub ignore: &'a [String],
    pub lang_filter: Option<Language>,
    pub config_main_branch: Option<&'a str>,
    pub gate_config: kiss::GateConfig,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RunTestOnceOutcome {
    Code(i32),
    Interrupted,
}

pub fn run_test(a: RunTestCmdArgs<'_>) -> i32 {
    match run_test_once(a) {
        RunTestOnceOutcome::Code(code) => code,
        RunTestOnceOutcome::Interrupted => 130,
    }
}

pub(crate) fn emit_test_progress(message: &str) {
    if !crate::test_runner::check_runtime_refresh::test_runner_stdout_enabled() {
        return;
    }
    emit_test_status(message);
}

pub(crate) fn emit_test_status(message: &str) {
    kiss::rust_llvm_cov_runner::emit_progress(message);
}

pub(crate) fn emit_stage_time(stage: &str, duration: std::time::Duration) {
    emit_test_progress(&format!(
        "kiss test: stage {stage} {}ms",
        duration.as_millis()
    ));
}

pub(crate) fn run_test_once(a: RunTestCmdArgs<'_>) -> RunTestOnceOutcome {
    crate::test_runner::runners::clear_python_collect_memo();

    let process_started = std::time::Instant::now();
    emit_test_progress("kiss test: Planning ...");
    match pipeline::run_overlapped_test(&a, process_started) {
        Ok(c) => RunTestOnceOutcome::Code(c),
        Err(e) => {
            if rust_batch_interrupt::consume_rust_batch_interrupted() {
                return RunTestOnceOutcome::Interrupted;
            }
            eprintln!("{e}");
            RunTestOnceOutcome::Code(1)
        }
    }
}

#[cfg(unix)]
pub(crate) use watch::control::{
    NudgeInvocation, NudgeRequestMsg, nudge_watcher_with_retry_on_wait, probe_live_watcher,
};
#[cfg(not(unix))]
pub(crate) use watch::nudge_kind::NudgeInvocation;
pub(crate) use watch::{WatchCoverageParams, WatchCoverageResult, WatchReloadSeed, run_test_watch};

#[cfg(test)]
#[allow(dead_code)]
fn plan_for_invocation(a: &RunTestCmdArgs<'_>) -> Result<PlannedSelectors, String> {
    match &a.invocation {
        TestInvocation::Commit => plan_selectors(PlanSelectorsRequest {
            mode: TestChangeMode::Commit,
            main_branch_cli: a.main_branch_cli,
            base_branch_cli: a.base_branch_cli,
            ignore: a.ignore,
            extras: crate::test_runner::language_keyed::LanguageKeyed {
                python: a.python_extra,
                rust: a.extra,
            },
            lang_filter: a.lang_filter,
            config_main_branch: a.config_main_branch,
        }),
        TestInvocation::Base => plan_selectors(PlanSelectorsRequest {
            mode: TestChangeMode::Base,
            main_branch_cli: a.main_branch_cli,
            base_branch_cli: a.base_branch_cli,
            ignore: a.ignore,
            extras: crate::test_runner::language_keyed::LanguageKeyed {
                python: a.python_extra,
                rust: a.extra,
            },
            lang_filter: a.lang_filter,
            config_main_branch: a.config_main_branch,
        }),
        TestInvocation::Main => plan_selectors(PlanSelectorsRequest {
            mode: TestChangeMode::Main,
            main_branch_cli: a.main_branch_cli,
            base_branch_cli: a.base_branch_cli,
            ignore: a.ignore,
            extras: crate::test_runner::language_keyed::LanguageKeyed {
                python: a.python_extra,
                rust: a.extra,
            },
            lang_filter: a.lang_filter,
            config_main_branch: a.config_main_branch,
        }),
        TestInvocation::All => plan_target_selectors(
            TargetPlanKind::All,
            a.ignore,
            crate::test_runner::language_keyed::LanguageKeyed {
                python: a.python_extra,
                rust: a.extra,
            },
            a.lang_filter,
            &a.gate_config,
        ),
        TestInvocation::Targets(targets) => plan_target_selectors(
            TargetPlanKind::Targets(targets.as_slice()),
            a.ignore,
            crate::test_runner::language_keyed::LanguageKeyed {
                python: a.python_extra,
                rust: a.extra,
            },
            a.lang_filter,
            &a.gate_config,
        ),
    }
}

mod pipeline;
mod plan;
mod rust_list_build;
pub(crate) mod workspace_selector_cache;
#[cfg(test)]
pub(crate) use plan::{
    PlanSelectorsRequest, TargetPlanKind, plan_selectors, plan_target_selectors,
};

#[cfg(test)]
pub(crate) mod test_mode_fixtures;

#[cfg(test)]
#[path = "explicit_test_targets_test.rs"]
mod explicit_test_targets_test;

#[cfg(test)]
#[path = "single_python_harness_timing_test.rs"]
mod single_python_harness_timing_test;

#[cfg(test)]
#[path = "python_named_target_args.rs"]
mod python_named_target_args;

#[cfg(test)]
#[path = "force_selected_python_e2e_test.rs"]
mod force_selected_python_e2e_test;

#[cfg(test)]
#[path = "force_all_population_test.rs"]
mod force_all_population_test;

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
#[path = "force_bad_test.rs"]
mod force_bad_test;

#[cfg(test)]
#[path = "planning_heartbeat_test.rs"]
mod planning_heartbeat_test;

#[cfg(test)]
#[path = "pipeline_progress_test.rs"]
mod pipeline_progress_test;

#[cfg(test)]
#[path = "pipeline_barrier_test.rs"]
mod pipeline_barrier_test;

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

#[cfg(test)]
#[path = "kt_target_types_test.rs"]
mod kt_target_types_test;
