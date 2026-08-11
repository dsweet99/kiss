//! Rust `LanguageRuntime` implementation.

use std::collections::BTreeMap;
use std::path::Path;

use kiss::Language;

use crate::test_runner::lang_iface::{
    AcceptMode, EnsureRequest, ExecutionWitness, LanguageRuntime, OutcomeBatch, PublishBatch,
    WitnessScope, summary_from_accepted_witness,
};
use crate::test_runner::runners::{
    SelectorExecutionSummary, kiss_test_report_id, rust_logical_to_kiss_test_ids,
};
use crate::test_runner::rust_coverage_index::{
    current_rust_coverage_batch_identity, repo_relative_coverage_file,
};

use super::witness_store::{
    PublishRustWitness, publish_rust_execution_witness, rust_identity_digest_from_batch,
    try_load_rust_execution_witness,
};

pub(crate) struct RustRuntime;

impl LanguageRuntime for RustRuntime {
    fn language(&self) -> Language {
        Language::Rust
    }

    fn current_identity(&self, request: &EnsureRequest) -> Result<String, String> {
        let identity =
            current_rust_coverage_batch_identity(&request.repo_root, &request.rust_extra)?;
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
        let publication = match request.mode {
            AcceptMode::All => Some(request.planned_rust.clone()),
            AcceptMode::Subset => Some(miss_set.to_vec()),
        };
        let summary = match request.mode {
            AcceptMode::All => {
                // Cov / Full population: CheckAggregate path (binary-level publish).
                crate::test_runner::rust_llvm_cov::run_rust_llvm_cov_check_aggregate_selectors(
                    &request.repo_root,
                    miss_set,
                    &request.rust_extra,
                    request.jobs,
                    None,
                    None,
                )?
            }
            AcceptMode::Subset => {
                // Test path Miss/repair: selector-entry exports.
                crate::test_runner::runners::run_rust_llvm_cov_selectors(
                    &request.repo_root,
                    miss_set,
                    &request.rust_extra,
                    request.force,
                    request.jobs,
                    publication.clone(),
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
            publication_universe: publication,
        })
    }

    fn publish_outcomes(
        &self,
        request: &EnsureRequest,
        batch: &PublishBatch,
    ) -> Result<(), String> {
        let identity =
            current_rust_coverage_batch_identity(&request.repo_root, &request.rust_extra)?;
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
        lines.extend(crate::test_runner::runners::build_rust_coverage_batch_dry_run_lines(
            selectors, extra, jobs,
        )?);
        Ok(lines)
    }

    fn accepted_summary(
        &self,
        request: &EnsureRequest,
        planned: &[String],
        witness: &ExecutionWitness,
    ) -> SelectorExecutionSummary {
        let report_ids =
            rust_logical_to_kiss_test_ids(&request.repo_root, &[]).unwrap_or_default();
        summary_from_accepted_witness(planned, witness, |selector| {
            kiss_test_report_id(&report_ids, selector)
        })
    }
}
