use std::collections::{BTreeMap, BTreeSet};

use crate::rpytest_runner::TestStatus;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_result::{
    RustCoverageBatchCounters, RustCoverageBatchResult,
};
use crate::rust_llvm_cov_runner::plan::batch_fingerprint::{
    RustCoverageBatchIdentity, RustCoverageToolIdentity, entry_fingerprint,
};
use crate::rust_llvm_cov_runner::plan::batch_plan::RustCoverageBatchRequest;
use crate::rust_llvm_cov_runner::{
    OrdinarySourceInvalidation, RustCovCacheStatus, RustLlvmCovError, RustLlvmCovOutcome,
    ValidatedCheckAggregate, classify_ordinary_source_delta, write_ordinary_source_snapshot,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SelectorMissReason {
    MissingEntry,
    NonPassed,
    EmptyCoverage,
    SourceDelta,
    Forced,
    NonCacheable,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PreparedRustBatch {
    pub hits_by_selector: BTreeMap<String, RustLlvmCovOutcome>,
    pub misses: Vec<String>,
    pub banned_or_terminal: BTreeMap<String, RustLlvmCovOutcome>,
    pub(crate) miss_reasons: BTreeMap<String, SelectorMissReason>,
}

impl PreparedRustBatch {
    pub fn misses_empty(&self) -> bool {
        self.misses.is_empty()
    }

    pub fn hit_result(&self, original_order: &[String]) -> RustCoverageBatchResult {
        merge_prepared(self, original_order, None)
    }
}

pub(crate) fn prepare_rust_batch(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
) -> Result<PreparedRustBatch, RustLlvmCovError> {
    let mut prepared = PreparedRustBatch::default();
    let source_invalid =
        classify_ordinary_source_delta(&req.cache_root, &req.source_root, identity);
    let forced: BTreeSet<&str> = req
        .force_rerun_selectors
        .iter()
        .map(String::as_str)
        .collect();
    let aggregate = crate::rust_llvm_cov_runner::load_current_check_aggregate_snapshot(
        &req.cache_root,
        &req.source_root,
        identity,
        None,
    )
    .map(|snapshot| snapshot.aggregate);
    for selector in &req.logical_selectors {
        classify_one_selector(
            req,
            tools,
            identity,
            selector,
            &forced,
            &source_invalid,
            aggregate.as_ref(),
            &mut prepared,
        );
    }
    if prepared.misses.is_empty() && !prepared.hits_by_selector.is_empty() {
        maybe_write_warm_seal(req, identity);
        let _ = write_ordinary_source_snapshot(&req.cache_root, &req.source_root, identity);
    }
    Ok(prepared)
}

#[allow(clippy::too_many_arguments)]
fn classify_one_selector(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    selector: &str,
    forced: &BTreeSet<&str>,
    source_invalid: &OrdinarySourceInvalidation,
    aggregate: Option<&ValidatedCheckAggregate>,
    prepared: &mut PreparedRustBatch,
) {
    if selector_timeout_is_ban(req, selector) {
        prepared
            .banned_or_terminal
            .insert(selector.to_string(), banned_timeout_outcome(selector));
    } else if let Some(reason) = miss_before_entry(req, selector, forced, source_invalid) {
        record_miss(prepared, selector, reason);
    } else {
        insert_hit_or_entry_miss(req, tools, identity, selector, aggregate, prepared);
    }
}

fn miss_before_entry(
    req: &RustCoverageBatchRequest,
    selector: &str,
    forced: &BTreeSet<&str>,
    source_invalid: &OrdinarySourceInvalidation,
) -> Option<SelectorMissReason> {
    if forced.contains(selector) {
        Some(SelectorMissReason::Forced)
    } else if req.cache_policy.is_non_cacheable(selector) {
        Some(SelectorMissReason::NonCacheable)
    } else if source_invalid.invalidates(selector) {
        Some(SelectorMissReason::SourceDelta)
    } else {
        None
    }
}

fn insert_hit_or_entry_miss(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    selector: &str,
    aggregate: Option<&ValidatedCheckAggregate>,
    prepared: &mut PreparedRustBatch,
) {
    let fingerprint = entry_fingerprint(&identity.input_digest, req, tools, selector);
    let Some(mut entry) = crate::rust_llvm_cov_runner::rust_cov_cache::load_rust_cov_cache_entry(
        &req.cache_root,
        &fingerprint,
    ) else {
        record_miss(prepared, selector, SelectorMissReason::MissingEntry);
        return;
    };
    if entry.status != TestStatus::Passed {
        record_miss(prepared, selector, SelectorMissReason::NonPassed);
        return;
    }
    if entry.coverage.files.is_empty()
        && let Some(aggregate) = aggregate
    {
        entry.coverage =
            crate::rust_llvm_cov_runner::selector_coverage_from_validated(aggregate, selector);
    }
    if entry.coverage.files.is_empty() {
        record_miss(prepared, selector, SelectorMissReason::EmptyCoverage);
        return;
    }
    prepared.hits_by_selector.insert(
        selector.to_string(),
        outcome_from_entry(entry, RustCovCacheStatus::Hit),
    );
}

pub(crate) fn merge_prepared(
    prepared: &PreparedRustBatch,
    original_order: &[String],
    fresh: Option<RustCoverageBatchResult>,
) -> RustCoverageBatchResult {
    let mut by_selector: BTreeMap<String, RustLlvmCovOutcome> = BTreeMap::new();
    by_selector.extend(prepared.hits_by_selector.clone());
    by_selector.extend(prepared.banned_or_terminal.clone());
    let mut counters = RustCoverageBatchCounters {
        cache_hits: prepared.hits_by_selector.len(),
        ..Default::default()
    };
    let mut batch_error = None;
    let mut test_binaries = Vec::new();
    if let Some(fresh) = fresh {
        for outcome in fresh.completed {
            by_selector.insert(outcome.selector.clone(), outcome);
        }
        counters.build_invocations = fresh.counters.build_invocations;
        counters.test_instances = fresh.counters.test_instances;
        counters.export_jobs = fresh.counters.export_jobs;
        counters.derived_state_published = fresh.counters.derived_state_published;
        counters.derived_repair = fresh.counters.derived_repair;
        counters.entry_generation_count = fresh.counters.entry_generation_count;
        counters.current_index_generation = fresh.counters.current_index_generation;
        counters.cache_pruned_entries = fresh.counters.cache_pruned_entries;
        counters.legacy_cleanup_deferred = fresh.counters.legacy_cleanup_deferred;
        counters.reverse_query_hits = fresh.counters.reverse_query_hits;
        counters.reverse_unavailable = fresh.counters.reverse_unavailable;
        counters.reverse_published = fresh.counters.reverse_published;
        counters.reverse_snapshots_reclaimed = fresh.counters.reverse_snapshots_reclaimed;
        batch_error = fresh.batch_error;
        test_binaries = fresh.test_binaries;
    }
    let completed = original_order
        .iter()
        .filter_map(|selector| by_selector.remove(selector))
        .collect();
    RustCoverageBatchResult {
        completed,
        batch_error,
        counters,
        test_binaries,
    }
}

fn record_miss(prepared: &mut PreparedRustBatch, selector: &str, reason: SelectorMissReason) {
    prepared.misses.push(selector.to_string());
    prepared.miss_reasons.insert(selector.to_string(), reason);
}

fn maybe_write_warm_seal(req: &RustCoverageBatchRequest, identity: &RustCoverageBatchIdentity) {
    if crate::rust_llvm_cov_runner::publish_derived::batch_entry_state::read_entry_state(
        &req.cache_root,
    )
    .is_some_and(|state| state.generation_fingerprint == identity.generation_fingerprint)
    {
        let _ = crate::rust_llvm_cov_runner::execute_or_reuse::batch_warm_hit_seal::write_warm_all_hit_seal(
            req, identity,
        );
    }
}

pub(crate) fn selector_timeout_is_ban(req: &RustCoverageBatchRequest, selector: &str) -> bool {
    req.selector_timeout_millis.get(selector) == Some(&0)
}

pub(crate) fn banned_timeout_outcome(selector: &str) -> RustLlvmCovOutcome {
    RustLlvmCovOutcome {
        selector: selector.to_string(),
        status: TestStatus::TimedOut,
        exit_code: Some(124),
        duration: std::time::Duration::ZERO,
        coverage: Default::default(),
        test_binary_ids: Vec::new(),
        cache_status: RustCovCacheStatus::FreshUnstored,
        stdout: None,
        stderr: None,
    }
}

pub(crate) fn outcome_from_entry(
    entry: crate::rust_llvm_cov_runner::rust_cov_cache::RustCovCacheEntry,
    cache_status: RustCovCacheStatus,
) -> RustLlvmCovOutcome {
    RustLlvmCovOutcome {
        selector: entry.selector,
        status: entry.status,
        exit_code: entry.exit_code,
        duration: entry.duration,
        coverage: entry.coverage,
        test_binary_ids: entry.test_binary_ids,
        cache_status,
        stdout: None,
        stderr: None,
    }
}

#[cfg(test)]
mod tests {
    use super::SelectorMissReason;

    #[test]
    fn miss_reason_variants_are_distinct() {
        assert_ne!(SelectorMissReason::MissingEntry, SelectorMissReason::Forced);
        assert_ne!(
            SelectorMissReason::SourceDelta,
            SelectorMissReason::EmptyCoverage
        );
        let _ = SelectorMissReason::NonPassed;
        let _ = SelectorMissReason::NonCacheable;
    }
}
