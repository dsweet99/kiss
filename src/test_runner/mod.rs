#[cfg(test)]
#[path = "capture_stdout.rs"]
pub(crate) mod capture_stdout;

pub(crate) mod check_line_coverage;
pub(crate) mod check_runtime_refresh;
mod coverage_decision;
pub(crate) mod duration;
pub(crate) mod ensure_runtime;
pub(crate) mod execution_witness;
pub(crate) mod lang_iface;
pub(crate) mod lang_python;
pub(crate) mod lang_rust;
pub(crate) mod last_status;
mod line_selection;
mod python_cache_path;
pub(crate) mod python_coverage_index;
pub(crate) mod coverage_index;
mod final_summary;
mod status_labels;
mod run_logic;
mod runners;
mod rust_coverage_index;
mod targets;
pub(crate) mod unit_test_timing;
mod rust_report_id_cache;
mod selector_ids;
mod language_keyed;
mod planned_selectors;
mod watch;
mod rust_batch_interrupt;
pub(crate) use planned_selectors::{
    PlannedSelectors, SelectorRunOptions, apply_cold_initialization_population,
    apply_force_all_population,
};
#[cfg(test)]
pub(crate) use planned_selectors::should_force_cold_initialization;

/// Compatibility path: llvm-cov adapters live under `lang_rust::llvm_cov`.
pub(crate) use lang_rust::llvm_cov as rust_llvm_cov;

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
    pub force_bad: bool,
    pub metrics: bool,
    pub jobs: usize,
    pub extra: &'a [String],
    /// Python-only pytest args: configured `-p` plugins plus CLI `extra`.
    pub python_extra: &'a [String],
    pub ignore: &'a [String],
    pub lang_filter: Option<Language>,
    pub config_main_branch: Option<&'a str>,
    /// Session gate from CLI config load; runtime must not reload independently.
    pub gate_config: kiss::GateConfig,
}

pub(crate) fn apply_force_bad(
    a: &RunTestCmdArgs<'_>,
    planned: &mut PlannedSelectors,
) -> Result<(), String> {
    if !a.force_bad {
        return Ok(());
    }
    let py_bad = runners::prior_failures_for_language(
        &planned.repo_root,
        Language::Python,
        a.python_extra,
    )?;
    let rs_bad = runners::prior_failures_for_language(
        &planned.repo_root,
        Language::Rust,
        a.extra,
    )?;
    let mut py = planned.prior_failure_selectors.python.clone();
    py.extend(py_bad.into_iter().map(|s| s.id));
    py.sort();
    py.dedup();
    planned.prior_failure_selectors.python = py;
    let mut rs = planned.prior_failure_selectors.rust.clone();
    rs.extend(rs_bad.into_iter().map(|s| s.id));
    rs.sort();
    rs.dedup();
    planned.prior_failure_selectors.rust = rs;
    // Also select bad tests that normal rules omitted (e.g. warm `kiss test .`).
    for sel in &planned.prior_failure_selectors.python {
        if !planned.sel.python.iter().any(|s| s == sel) {
            planned.sel.python.push(sel.clone());
        }
    }
    for sel in &planned.prior_failure_selectors.rust {
        if !planned.sel.rust.iter().any(|s| s == sel) {
            planned.sel.rust.push(sel.clone());
        }
    }
    Ok(())
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
    use std::io::Write;
    println!("{message}");
    let _ = std::io::stdout().flush();
}

pub(crate) fn run_test_once(a: RunTestCmdArgs<'_>) -> RunTestOnceOutcome {
    let dry_run = a.dry_run;
    let force_rerun = a.force_rerun;
    let metrics = a.metrics;
    let jobs = a.jobs;
    let extra = a.extra;
    let python_extra = a.python_extra;
    // Emit before planning so long selector/coverage work is not silent.
    emit_test_progress("kiss test: Planning ...");
    let plan_started = std::time::Instant::now();
    match plan_for_invocation(&a) {
        Ok(mut planned) => {
            apply_cold_initialization_population(&a, &mut planned);
            apply_force_all_population(&a, &mut planned);
            if let Err(e) = apply_force_bad(&a, &mut planned) {
                eprintln!("{e}");
                return RunTestOnceOutcome::Code(1);
            }
            match run_selectors(
                &planned,
                SelectorRunOptions {
                    dry_run,
                    force_rerun,
                    metrics,
                    jobs,
                    extras: crate::test_runner::language_keyed::LanguageKeyed {
                        python: python_extra,
                        rust: extra,
                    },
                    plan_duration: plan_started.elapsed(),
                    gate: a.gate_config.clone(),
                },
            ) {
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
        Err(e) => {
            eprintln!("{e}");
            RunTestOnceOutcome::Code(1)
        }
    }
}

pub(crate) use watch::run_test_watch;
pub(crate) use watch::{enter_watch_background, watch_background_active};

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
        ),
        TestInvocation::Targets(targets) => plan_target_selectors(
            TargetPlanKind::Targets(targets.as_slice()),
            a.ignore,
            crate::test_runner::language_keyed::LanguageKeyed {
                python: a.python_extra,
                rust: a.extra,
            },
            a.lang_filter,
        ),
    }
}

mod plan;
mod workspace_selector_cache;
pub(crate) use plan::{PlanSelectorsRequest, TargetPlanKind, plan_selectors, plan_target_selectors};

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
#[path = "planning_heartbeat_test.rs"]
mod planning_heartbeat_test;

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
