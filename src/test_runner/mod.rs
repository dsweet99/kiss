#![cfg_attr(not(test), allow(dead_code))]

#[cfg(test)]
#[path = "capture_stdout.rs"]
pub(crate) mod capture_stdout;

pub(crate) mod duration;
pub(crate) mod ensure_runtime;
pub(crate) mod execution_witness;
pub(crate) mod final_summary;
pub(crate) mod lang_iface;
pub(crate) mod lang_python;
pub(crate) mod lang_rust;
pub(crate) mod language_keyed;
mod planned_selectors;
mod run_logic;
mod runners;
mod rust_batch_interrupt;
mod rust_report_id_cache;
mod selector_ids;
mod status_labels;
pub(crate) mod target_request;
mod targets;
mod test_selection;
pub(crate) use targets::expand_target_operands;
pub(crate) mod tests_remaining;
pub(crate) mod unit_test_timing;
pub(crate) mod universe_root;
#[cfg(test)]
pub(crate) use planned_selectors::should_force_cold_initialization;
pub(crate) use planned_selectors::{PlannedSelectors, SelectorRunOptions, empty_planned};
#[cfg(test)]
pub(crate) use planned_selectors::{
    apply_cold_initialization_population, apply_force_all_population,
};
pub(crate) use rust_batch_interrupt::consume_rust_batch_interrupted;

use kiss::Language;

use crate::bin_cli::args::TestInvocation;
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
    #[cfg_attr(not(test), allow(dead_code))]
    pub invocation: TestInvocation,
    pub(crate) target_request: target_request::TargetRequest,
    pub main_branch_cli: Option<&'a str>,
    pub base_branch_cli: Option<&'a str>,
    pub dry_run: bool,
    pub force_rerun: bool,
    pub metrics: bool,
    pub jobs: usize,
    pub extras: language_keyed::LanguageKeyed<&'a [String]>,
    pub config_main_branch: Option<&'a str>,
    pub gate_config: kiss::GateConfig,
    pub(crate) doubles: Option<std::sync::Arc<pipeline::PipelineDoubles>>,
}

impl RunTestCmdArgs<'_> {
    pub(crate) fn lang_filter(&self) -> Option<Language> {
        self.target_request.language()
    }

    pub(crate) fn ignore(&self) -> &[String] {
        &self.target_request.ignore
    }
}

#[cfg(test)]
impl RunTestCmdArgs<'_> {
    pub(crate) fn set_invocation(&mut self, invocation: TestInvocation) {
        self.invocation = invocation;
        self.refresh_target_request();
    }

    pub(crate) fn set_lang_filter(&mut self, lang_filter: Option<Language>) {
        self.target_request.set_language(lang_filter);
        self.invocation = target_request::to_compat_invocation(&self.target_request);
    }

    fn refresh_target_request(&mut self) {
        let request = target_request::request_from_invocation(
            &self.invocation,
            self.main_branch_cli,
            self.base_branch_cli,
            self.config_main_branch,
            self.target_request.language(),
            &self.target_request.ignore,
        );
        self.invocation = target_request::to_compat_invocation(&request);
        self.target_request = request;
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RunTestOnceOutcome {
    Code(i32),
    Interrupted,
    EngineError(String),
}

#[cfg(test)]
pub fn run_test(a: RunTestCmdArgs<'_>) -> i32 {
    match run_test_once(a) {
        RunTestOnceOutcome::Code(code) => code,
        RunTestOnceOutcome::Interrupted => 130,
        RunTestOnceOutcome::EngineError(_) => 1,
    }
}

pub(crate) fn test_state_dir(repo_root: &std::path::Path) -> std::path::PathBuf {
    kiss::test_state_dir(repo_root)
}

pub(crate) fn emit_test_progress(message: &str) {
    emit_test_status(message);
}

pub(crate) fn emit_test_status(message: &str) {
    kiss::test_progress::emit_progress(message);
}

pub(crate) fn emit_stage_time(stage: &str, duration: std::time::Duration) {
    emit_test_progress(&format!(
        "kiss test: stage {stage} {}ms",
        duration.as_millis()
    ));
}

pub(crate) fn run_test_once(a: RunTestCmdArgs<'_>) -> RunTestOnceOutcome {
    match run_live_overlapped_test(&a, std::time::Instant::now()) {
        Ok(code) => RunTestOnceOutcome::Code(code),
        Err(err) => {
            if consume_rust_batch_interrupted() {
                return RunTestOnceOutcome::Interrupted;
            }
            let line = kiss_test_engine_line(err);
            eprintln!("{line}");
            RunTestOnceOutcome::EngineError(line)
        }
    }
}

fn kiss_test_engine_line(err: String) -> String {
    if err.starts_with("error: kiss test:") {
        err
    } else {
        format!("error: kiss test: {err}")
    }
}

pub(crate) fn run_live_overlapped_test(
    a: &RunTestCmdArgs<'_>,
    process_started: std::time::Instant,
) -> Result<i32, String> {
    crate::test_runner::runners::clear_python_collect_memo();
    crate::test_runner::tests_remaining::reset_tests_remaining();
    let _progress_watchdog = kiss::test_progress::ProgressWatchdog::start();
    pipeline::run_overlapped_test(a, process_started)
}

#[cfg(test)]
fn plan_for_invocation(a: &RunTestCmdArgs<'_>) -> Result<PlannedSelectors, String> {
    use crate::test_runner::target_request::{TargetFocus, operand_raws};
    let extras = a.extras;
    match &a.target_request.focus {
        TargetFocus::Git(_) => plan_selectors(pipeline::git_plan_request(a)),
        TargetFocus::Workspace => plan_target_selectors(
            TargetPlanKind::All,
            a.ignore(),
            extras,
            a.lang_filter(),
            &a.gate_config,
        ),
        TargetFocus::Operands(_) => {
            let targets = operand_raws(&a.target_request.focus).unwrap_or_default();
            plan_target_selectors(
                TargetPlanKind::Targets(targets.as_slice()),
                a.ignore(),
                extras,
                a.lang_filter(),
                &a.gate_config,
            )
        }
    }
}

mod lang_registry;
mod pipeline;
mod plan;
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
#[path = "runners_test.rs"]
mod runners_test;

#[cfg(test)]
#[path = "runners_workspace_test.rs"]
mod runners_workspace_test;

#[cfg(test)]
#[path = "runners_request_test.rs"]
mod runners_request_test;

#[cfg(test)]
#[path = "test_cli_acceptance_test.rs"]
mod test_cli_acceptance_test;

#[cfg(test)]
#[path = "kt_target_types_test.rs"]
mod kt_target_types_test;
