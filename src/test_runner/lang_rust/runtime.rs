use std::collections::BTreeMap;
use std::path::Path;

use kiss::Language;

use crate::test_runner::lang_iface::{
    AcceptMode, EnsureRequest, ExecutionWitness, LanguageRuntime, OutcomeBatch, PublishBatch,
    SourceDeltaMisses, WitnessScope, summary_from_accepted_witness,
};
use crate::test_runner::runners::SelectorExecutionSummary;
use crate::test_runner::rust_coverage_index::{
    current_rust_coverage_batch_identity, repo_relative_coverage_file,
};
use crate::test_runner::selector_ids::report_string_for_logical_string;

use super::witness_store::{
    PublishRustWitness, publish_rust_execution_witness, rust_identity_digest_from_batch,
    try_load_rust_execution_witness,
};

#[path = "population_repair.rs"]
mod population_repair;

pub(crate) struct RustRuntime;

fn rust_population_publication_selectors(
    mode: AcceptMode,
    planned: &[String],
) -> Option<Vec<String>> {
    match mode {
        AcceptMode::All => Some(planned.to_vec()),
        AcceptMode::Subset => None,
    }
}

fn rust_summary_from_witness(
    request: &EnsureRequest,
    planned: &[String],
    witness: &ExecutionWitness,
) -> SelectorExecutionSummary {
    if !kiss::time_gate_uses_path_prefixes(&request.gate.max_unit_test_seconds) {
        return summary_from_accepted_witness(planned, witness, str::to_string);
    }
    let report_ids = crate::test_runner::rust_report_id_cache::rust_logical_to_kiss_test_ids_cached(
        &request.repo_root,
        &[],
    )
    .unwrap_or_default();
    summary_from_accepted_witness(planned, witness, |selector| {
        report_string_for_logical_string(&report_ids, selector)
    })
}

fn rust_summary_from_witness_statuses(
    request: &EnsureRequest,
    planned: &[String],
    witness: &ExecutionWitness,
) -> SelectorExecutionSummary {
    if !kiss::time_gate_uses_path_prefixes(&request.gate.max_unit_test_seconds) {
        return crate::test_runner::lang_iface::summary_from_witness_statuses(
            planned,
            witness,
            str::to_string,
            false,
        );
    }
    let report_ids = crate::test_runner::rust_report_id_cache::rust_logical_to_kiss_test_ids_cached(
        &request.repo_root,
        &[],
    )
    .unwrap_or_default();
    crate::test_runner::lang_iface::summary_from_witness_statuses(
        planned,
        witness,
        |selector| report_string_for_logical_string(&report_ids, selector),
        false,
    )
}

fn rust_publication_universe(
    mode: AcceptMode,
    planned: &[String],
    miss_set: &[String],
) -> Option<Vec<String>> {
    match mode {
        AcceptMode::All => Some(planned.to_vec()),
        AcceptMode::Subset => Some(miss_set.to_vec()),
    }
}

impl SourceDeltaMisses for RustRuntime {
    fn extra_source_delta_misses(
        &self,
        request: &EnsureRequest,
        planned: &[String],
    ) -> Result<Vec<String>, String> {
        super::rust_source_delta_misses(&request.repo_root, planned, &request.extras.rust)
    }
}

impl LanguageRuntime for RustRuntime {
    fn language(&self) -> Language {
        Language::Rust
    }

    fn current_identity(&self, request: &EnsureRequest) -> Result<String, String> {
        let identity =
            current_rust_coverage_batch_identity(&request.repo_root, &request.extras.rust)?;
        Ok(rust_identity_digest_from_batch(&identity))
    }

    fn load_full_witness(&self, repo_root: &Path) -> Result<ExecutionWitness, String> {
        try_load_rust_execution_witness(repo_root)
    }

    fn run_selectors(
        &self,
        request: &EnsureRequest,
        miss_set: &[String],
    ) -> Result<OutcomeBatch, String> {
        if miss_set.is_empty() {
            return Ok(OutcomeBatch::default());
        }
        let publication_universe =
            rust_publication_universe(request.mode, &request.planned.rust, miss_set);
        let summary = match request.mode {
            AcceptMode::All => {

                crate::test_runner::rust_llvm_cov::run_rust_llvm_cov_check_aggregate_selectors_with_gate(
                    &request.repo_root,
                    miss_set,
                    &request.extras.rust,
                    request.jobs,
                    None,
                    None,
                    &request.gate,
                )?
            }
            AcceptMode::Subset => {

                crate::test_runner::runners::run_rust_llvm_cov_selectors(
                    &request.repo_root,
                    miss_set,
                    &request.extras.rust,
                    request.force,
                    &request.force_selectors,
                    request.jobs,
                    rust_population_publication_selectors(request.mode, &request.planned.rust),
                    &request.gate,
                )?
            }
        };
        let (statuses, durations_ns) =
            super::publish_merge::statuses_from_summary(&summary, miss_set);
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
        let identity =
            current_rust_coverage_batch_identity(&request.repo_root, &request.extras.rust)?;
        let prior = try_load_rust_execution_witness(&request.repo_root).ok();
        let universe = super::publish_merge::publication_universe(batch, prior.as_ref());
        let (statuses, durations) =
            super::publish_merge::merge_statuses(&universe, prior.as_ref(), batch);
        let covered = super::publish_merge::covered_sets_for_publish(batch, prior.as_ref());
        let complete =
            super::publish_merge::publish_complete(request, &universe, &statuses, prior.as_ref());
        publish_rust_execution_witness(PublishRustWitness {
            repo_root: &request.repo_root,
            identity: &identity,
            scope: WitnessScope::Full,
            selectors: &universe,
            statuses: &statuses,
            durations_ns: &durations,
            covered_lines: &covered,
            complete,
            jobs: request.jobs,
        })?;
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
        jobs: usize,
    ) -> Result<Vec<String>, String> {
        let mut lines = Vec::new();
        if population {
            lines.push("RUST COVERAGE POPULATION".to_string());
        }
        lines.extend(
            crate::test_runner::runners::build_rust_coverage_batch_dry_run_lines(
                selectors, extra, jobs,
            )?,
        );
        Ok(lines)
    }

    fn accepted_summary(
        &self,
        request: &EnsureRequest,
        planned: &[String],
        witness: &ExecutionWitness,
    ) -> Result<SelectorExecutionSummary, String> {
        let mut summary = rust_summary_from_witness(request, planned, witness);
        if population_repair::repair_stale_population_on_all_mode_accept(request, planned)? {
            summary.rust_derived_repair = true;
        }
        Ok(summary)
    }

    fn cached_witness_summary(
        &self,
        request: &EnsureRequest,
        planned: &[String],
        witness: &ExecutionWitness,
    ) -> SelectorExecutionSummary {
        rust_summary_from_witness_statuses(request, planned, witness)
    }

    fn selectors_for_time_gate(
        &self,
        request: &EnsureRequest,
        selectors: &[String],
    ) -> Result<Vec<String>, String> {
        if !kiss::time_gate_uses_path_prefixes(&request.gate.max_unit_test_seconds) {
            return Ok(selectors.to_vec());
        }
        let report_ids =
            crate::test_runner::rust_report_id_cache::rust_logical_to_kiss_test_ids_cached(
                &request.repo_root,
                &[],
            )?;
        for selector in selectors {
            crate::test_runner::runners::require_kiss_test_report_id(&report_ids, selector)?;
        }
        Ok(
            crate::test_runner::selector_ids::report_strings_for_logical_strings(
                &report_ids,
                selectors,
            ),
        )
    }
}

#[cfg(test)]
mod publication_selector_tests {
    use super::{rust_population_publication_selectors, rust_publication_universe};
    use crate::test_runner::lang_iface::AcceptMode;

    #[test]
    fn subset_does_not_publish_a_selective_miss_set_as_population() {
        let planned = vec!["a".into(), "b".into()];
        let misses = vec!["a".into()];
        assert_eq!(
            rust_population_publication_selectors(AcceptMode::Subset, &planned),
            None
        );
        assert_eq!(
            rust_publication_universe(AcceptMode::Subset, &planned, &misses),
            Some(misses)
        );
    }

    #[test]
    fn all_mode_publishes_the_planned_universe() {
        let planned = vec!["a".into(), "b".into()];
        assert_eq!(
            rust_population_publication_selectors(AcceptMode::All, &planned),
            Some(planned.clone())
        );
        assert_eq!(
            rust_publication_universe(AcceptMode::All, &planned, &["a".into()]),
            Some(planned)
        );
    }
}

impl crate::test_runner::coverage_decision::SupportedLanguage for RustRuntime {
    fn language(&self) -> Language {
        Language::Rust
    }
}

#[cfg(test)]
#[path = "population_repair_test.rs"]
mod population_repair_tests;
