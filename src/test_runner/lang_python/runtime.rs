use std::collections::BTreeMap;
use std::path::Path;

use kiss::Language;
use kiss::rpytest_runner::TestStatus;

use crate::test_runner::lang_iface::{
    EnsureRequest, ExecutionWitness, LanguageRuntime, OutcomeBatch, PublishBatch, WitnessStatus,
    summary_from_accepted_witness,
};
use crate::test_runner::python_coverage_index::generation::{
    current_python_execution_identity, identity_matches_current,
};
use crate::test_runner::python_coverage_index::{
    GenerationReason, publish_python_derived_state_with_filter,
    repair_python_population_generation, repo_relative_coverage_file,
    selector_deltas_from_cached_outcomes, try_load_pinned_python_generation_warm,
};
use crate::test_runner::runners::SelectorExecutionSummary;

use super::witness_view::{python_identity_digest, python_witness_from_pinned};

pub(crate) struct PythonRuntime;

impl LanguageRuntime for PythonRuntime {
    fn language(&self) -> Language {
        Language::Python
    }

    fn current_identity(&self, request: &EnsureRequest) -> Result<String, String> {
        if let Ok(pinned) = try_load_pinned_python_generation_warm(&request.repo_root)
            && identity_matches_current(
                &request.repo_root,
                &pinned.plan.base_identity,
                &request.extras.python,
            )
        {
            return Ok(python_identity_digest(&pinned));
        }
        let exec = current_python_execution_identity(&request.repo_root, &request.extras.python)?;
        Ok(format!("py:{}:pending", exec.input_fingerprint))
    }

    fn load_full_witness(&self, repo_root: &Path) -> Result<ExecutionWitness, String> {
        let pinned = try_load_pinned_python_generation_warm(repo_root)
            .map_err(|e| format!("python witness load: {e:?}"))?;
        Ok(python_witness_from_pinned(&pinned))
    }

    fn run_selectors(
        &self,
        request: &EnsureRequest,
        miss_set: &[String],
    ) -> Result<OutcomeBatch, String> {
        if miss_set.is_empty() {
            return Ok(OutcomeBatch::default());
        }
        let summary = crate::test_runner::runners::run_rslip_selectors(
            &request.repo_root,
            miss_set,
            &request.extras.python,
            request.force,
            &[],
            request.jobs,
            None,
            &request.gate,
        )?;
        let (statuses, durations_ns) = statuses_from_summary(&summary, miss_set);

        let publication_universe = match request.mode {
            crate::test_runner::lang_iface::AcceptMode::All
                if miss_set.len() == request.planned.python.len() =>
            {
                Some(request.planned.python.clone())
            }
            _ => None,
        };
        Ok(OutcomeBatch {
            summary,
            selectors: miss_set.to_vec(),
            statuses,
            durations_ns,
            covered_lines: BTreeMap::new(),
            publication_universe,
        })
    }

    fn publish_outcomes(
        &self,
        request: &EnsureRequest,
        batch: &PublishBatch,
    ) -> Result<(), String> {
        let is_indexable = |path: &Path, repo_root: &Path| {
            repo_relative_coverage_file(repo_root, &path.to_string_lossy()).is_some()
        };
        if let Some(universe) = batch.publication_universe.as_ref() {
            let started = std::time::Instant::now();
            let restamped = !request.force
                && crate::test_runner::lang_python::generation::try_restamp_matching_pinned_universe(
                    &request.repo_root,
                    universe,
                    &request.extras.python,
                    &is_indexable,
                    Some(batch.summary.cache_miss_selectors.as_slice()),
                )?;
            if !restamped {
                publish_python_derived_state_with_filter(
                    &request.repo_root,
                    Some(universe),
                    &request.extras.python,
                    is_indexable,
                )?;
            }
            crate::test_runner::emit_stage_time("python_generation_publish", started.elapsed());
        } else {
            let started = std::time::Instant::now();
            let deltas = selector_deltas_from_cached_outcomes(
                &request.repo_root,
                &batch.selectors,
                &request.extras.python,
                &is_indexable,
                &request.gate,
            )?;
            let _ = repair_python_population_generation(
                &request.repo_root,
                &deltas,
                GenerationReason::IncompleteRepair,
            )?;
            crate::test_runner::emit_stage_time("selective_index_repair", started.elapsed());
        }
        crate::test_runner::python_coverage_index::clear_python_generation_warm_memo();
        Ok(())
    }

    fn is_indexable_source(&self, path: &Path, repo_root: &Path) -> bool {
        repo_relative_coverage_file(repo_root, &path.to_string_lossy()).is_some()
    }

    fn dry_run_lines(
        &self,
        selectors: &[String],
        population: bool,
        extra: &[String],
        _jobs: usize,
    ) -> Result<Vec<String>, String> {
        let mut lines = Vec::new();
        if population {
            lines.push("PYTHON COVERAGE POPULATION".to_string());
        }
        if !selectors.is_empty() {
            let argv = crate::test_runner::runners::build_pytest_argv(selectors, extra);
            lines.push(crate::test_runner::runners::shell_quote_line(&argv));
        }
        Ok(lines)
    }

    fn accepted_summary(
        &self,
        _request: &EnsureRequest,
        planned: &[String],
        witness: &ExecutionWitness,
    ) -> SelectorExecutionSummary {
        summary_from_accepted_witness(planned, witness, |selector| selector.to_string())
    }
}

fn statuses_from_summary(
    summary: &SelectorExecutionSummary,
    selectors: &[String],
) -> (Vec<WitnessStatus>, Vec<Option<u64>>) {
    let mut statuses = Vec::with_capacity(selectors.len());
    let mut durations = Vec::with_capacity(selectors.len());
    for sel in selectors {
        let status = match summary.raw_statuses.get(sel).copied().unwrap_or_else(|| {
            if summary.timed_out_selectors.iter().any(|s| s == sel) {
                TestStatus::TimedOut
            } else if summary.failed_selectors.iter().any(|s| s == sel) {
                TestStatus::Failed
            } else {
                TestStatus::Passed
            }
        }) {
            TestStatus::TimedOut => WitnessStatus::TimedOut,
            TestStatus::Failed => WitnessStatus::Failed,
            TestStatus::Passed => WitnessStatus::Passed,
        };
        statuses.push(status);

        durations.push(summary.selector_durations_ns.get(sel).copied());
    }
    (statuses, durations)
}

impl crate::test_runner::coverage_decision::SupportedLanguage for PythonRuntime {
    fn language(&self) -> Language {
        Language::Python
    }
}
