//! Publish Full Rust execution witnesses after successful batches.
//!
//! CheckAggregate incremental repairs must merge into an existing Full witness.
//! They must never replace a Full pointer with the repair subset alone.

use std::collections::BTreeMap;
use std::path::Path;

use rust_llvm_cov_runner::{CoverageOutputMode, RustCoverageBatchIdentity, RustCoverageBatchRequest};

use crate::test_runner::execution_witness::{
    ExecutionWitness, PublishRustWitness, WitnessScope, WitnessStatus,
    publish_rust_execution_witness, rust_identity_digest_from_batch, try_load_rust_execution_witness,
};
use crate::test_runner::runners::{
    SelectorExecutionSummary, kiss_test_report_id, rust_logical_to_kiss_test_ids,
};

pub(super) fn publish_rust_witness_after_batch(
    repo_root: &Path,
    batch_req: &RustCoverageBatchRequest,
    summary: &SelectorExecutionSummary,
) -> Result<(), String> {
    if summary.failed > 0 || !summary.timed_out_selectors.is_empty() || summary.total == 0 {
        return Ok(());
    }
    let batch_identity =
        crate::test_runner::rust_coverage_index::current_rust_coverage_batch_identity(
            repo_root,
            &batch_req.test_args,
        )?;
    let existing_full = load_matching_full_witness(repo_root, &batch_identity);
    let by_logical =
        merged_statuses(repo_root, batch_req, summary, existing_full.as_ref());
    let Some(selectors) = full_publication_selectors(
        batch_req,
        repo_root,
        &batch_identity,
        existing_full.as_ref(),
        &by_logical,
    ) else {
        return Ok(());
    };
    let Some((statuses, durations_ns)) = align_statuses(
        repo_root,
        &selectors,
        &by_logical,
        summary,
        existing_full.as_ref(),
    ) else {
        // Incomplete relative to the Full universe: leave the Full pointer unchanged.
        return Ok(());
    };
    let all_passed = statuses.iter().all(|s| *s == WitnessStatus::Passed);
    let covered_lines = capture_covered_lines(
        repo_root,
        batch_req,
        &batch_identity,
        &selectors,
        existing_full.as_ref(),
    );
    let _ = publish_rust_execution_witness(PublishRustWitness {
        repo_root,
        identity: &batch_identity,
        scope: WitnessScope::Full,
        selectors: &selectors,
        statuses: &statuses,
        durations_ns: &durations_ns,
        covered_lines: &covered_lines,
        complete: all_passed,
    })?;
    Ok(())
}

fn load_matching_full_witness(
    repo_root: &Path,
    batch_identity: &RustCoverageBatchIdentity,
) -> Option<ExecutionWitness> {
    let witness = try_load_rust_execution_witness(repo_root).ok()?;
    (witness.scope == WitnessScope::Full
        && witness.identity_digest == rust_identity_digest_from_batch(batch_identity))
    .then_some(witness)
}

/// Decide the Full selector universe for publication. Returns `None` when this
/// batch must not claim or overwrite Full.
pub(super) fn full_publication_selectors(
    batch_req: &RustCoverageBatchRequest,
    repo_root: &Path,
    batch_identity: &RustCoverageBatchIdentity,
    existing_full: Option<&ExecutionWitness>,
    by_logical: &BTreeMap<String, WitnessStatus>,
) -> Option<Vec<String>> {
    let is_check_aggregate = matches!(
        batch_req.coverage_output_mode,
        CoverageOutputMode::CheckAggregate { .. }
    );

    if let Some(publication) = batch_req.population_publication_selectors.as_ref() {
        return population_full_selectors(publication, existing_full, by_logical);
    }

    if is_check_aggregate {
        return check_aggregate_full_selectors(
            batch_req,
            repo_root,
            batch_identity,
            existing_full,
            by_logical,
        );
    }

    // Selective: only publish Full when this run covers the current population
    // or merges into an existing Full witness.
    let mut selectors = batch_req.logical_selectors.clone();
    if let Some(existing) = existing_full {
        for sel in &existing.selectors {
            if !selectors.contains(sel) {
                selectors.push(sel.clone());
            }
        }
    }
    selectors.sort();
    selectors.dedup();
    let covers_population = population_equals(
        &batch_req.cache_root,
        repo_root,
        batch_identity,
        &selectors,
    );
    if covers_population || existing_full.is_some() {
        Some(selectors)
    } else {
        None
    }
}

fn population_full_selectors(
    publication: &[String],
    existing_full: Option<&ExecutionWitness>,
    by_logical: &BTreeMap<String, WitnessStatus>,
) -> Option<Vec<String>> {
    let mut selectors = publication.to_vec();
    if let Some(existing) = existing_full {
        // Never shrink below an existing same-identity Full universe.
        for sel in &existing.selectors {
            if !selectors.contains(sel) {
                selectors.push(sel.clone());
            }
        }
    }
    selectors.sort();
    selectors.dedup();
    if selectors.iter().all(|s| by_logical.contains_key(s)) {
        Some(selectors)
    } else {
        None
    }
}

fn check_aggregate_full_selectors(
    batch_req: &RustCoverageBatchRequest,
    repo_root: &Path,
    batch_identity: &RustCoverageBatchIdentity,
    existing_full: Option<&ExecutionWitness>,
    by_logical: &BTreeMap<String, WitnessStatus>,
) -> Option<Vec<String>> {
    // Prefer an existing Full universe (membership-compatible repair merge).
    if let Some(existing) = existing_full {
        let mut selectors = existing.selectors.clone();
        for sel in &batch_req.logical_selectors {
            if !selectors.contains(sel) {
                selectors.push(sel.clone());
            }
        }
        selectors.sort();
        selectors.dedup();
        return Some(selectors);
    }
    // No Full base: only claim Full when this batch equals the current population.
    let mut selectors = batch_req.logical_selectors.clone();
    selectors.sort();
    selectors.dedup();
    if !population_equals(
        &batch_req.cache_root,
        repo_root,
        batch_identity,
        &selectors,
    ) {
        return None;
    }
    if selectors.iter().all(|s| by_logical.contains_key(s)) {
        Some(selectors)
    } else {
        None
    }
}

fn population_equals(
    cache_root: &Path,
    repo_root: &Path,
    batch_identity: &RustCoverageBatchIdentity,
    selectors: &[String],
) -> bool {
    rust_llvm_cov_runner::load_current_population_state(
        cache_root,
        repo_root,
        batch_identity,
        Some(selectors),
    )
    .is_some_and(|pop| {
        let mut pop_sel = pop.selectors;
        pop_sel.sort();
        pop_sel.dedup();
        pop_sel == selectors
    })
}

pub(super) fn merged_statuses(
    repo_root: &Path,
    batch_req: &RustCoverageBatchRequest,
    summary: &SelectorExecutionSummary,
    existing_full: Option<&ExecutionWitness>,
) -> BTreeMap<String, WitnessStatus> {
    let report_ids = rust_logical_to_kiss_test_ids(repo_root, &[]).unwrap_or_default();
    let mut by_logical = BTreeMap::new();
    if let Some(existing) = existing_full {
        for (sel, st) in existing.selectors.iter().zip(existing.statuses.iter()) {
            by_logical.insert(sel.clone(), *st);
        }
    }
    for logical in &batch_req.logical_selectors {
        let report = kiss_test_report_id(&report_ids, logical);
        let status = if summary.failed_selectors.iter().any(|s| s == &report) {
            WitnessStatus::Failed
        } else if summary.timed_out_selectors.iter().any(|s| s == &report) {
            WitnessStatus::TimedOut
        } else {
            WitnessStatus::Passed
        };
        by_logical.insert(logical.clone(), status);
    }
    by_logical
}

pub(super) fn align_statuses(
    repo_root: &Path,
    selectors: &[String],
    by_logical: &BTreeMap<String, WitnessStatus>,
    summary: &SelectorExecutionSummary,
    existing_full: Option<&ExecutionWitness>,
) -> Option<(Vec<WitnessStatus>, Vec<u64>)> {
    let report_ids = rust_logical_to_kiss_test_ids(repo_root, &[]).unwrap_or_default();
    let mut prior_durations: BTreeMap<&str, u64> = BTreeMap::new();
    if let Some(existing) = existing_full {
        for (sel, dur) in existing.selectors.iter().zip(existing.durations_ns.iter()) {
            prior_durations.insert(sel.as_str(), *dur);
        }
    }
    let mut statuses = Vec::with_capacity(selectors.len());
    let mut durations_ns = Vec::with_capacity(selectors.len());
    for sel in selectors {
        let st = by_logical.get(sel)?;
        statuses.push(*st);
        let report = kiss_test_report_id(&report_ids, sel);
        let dur = summary
            .selector_durations_ns
            .get(sel)
            .or_else(|| summary.selector_durations_ns.get(&report))
            .copied()
            .or_else(|| prior_durations.get(sel.as_str()).copied())
            .unwrap_or(0);
        durations_ns.push(dur);
    }
    Some((statuses, durations_ns))
}

fn capture_covered_lines(
    repo_root: &Path,
    batch_req: &RustCoverageBatchRequest,
    batch_identity: &RustCoverageBatchIdentity,
    selectors: &[String],
    existing_full: Option<&ExecutionWitness>,
) -> std::collections::BTreeMap<String, std::collections::BTreeSet<u32>> {
    use std::collections::{BTreeMap, BTreeSet};
    if let Some(snapshot) = rust_llvm_cov_runner::load_current_check_aggregate_snapshot(
        &batch_req.cache_root,
        repo_root,
        batch_identity,
        Some(selectors),
    ) {
        return snapshot.covered_lines;
    }
    if let Some(snapshot) = rust_llvm_cov_runner::load_current_generation_coverage_snapshot(
        &batch_req.cache_root,
        repo_root,
        batch_identity,
        Some(selectors),
    ) {
        return snapshot.covered_lines;
    }
    let mut from_prior = BTreeMap::<String, BTreeSet<u32>>::new();
    if let Some(existing) = existing_full {
        for (path, lines) in &existing.covered_lines {
            from_prior.insert(path.clone(), lines.iter().copied().collect());
        }
    }
    from_prior
}

#[cfg(test)]
#[path = "witness_test.rs"]
mod rust_llvm_cov_witness_test;
