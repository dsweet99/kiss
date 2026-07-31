//! Reverse-index fields on Rust batch counter recording.

use super::SelectorExecutionSummary;
use rust_llvm_cov_runner::RustCoverageBatchCounters;

pub(super) fn record_reverse_batch_counters(
    summary: &mut SelectorExecutionSummary,
    counters: &RustCoverageBatchCounters,
) {
    summary.rust_reverse_query_hits += counters.reverse_query_hits;
    summary.rust_reverse_unavailable_schema += counters.reverse_unavailable.schema;
    summary.rust_reverse_unavailable_generation += counters.reverse_unavailable.generation;
    summary.rust_reverse_unavailable_revision += counters.reverse_unavailable.revision;
    summary.rust_reverse_unavailable_fingerprint += counters.reverse_unavailable.fingerprint;
    summary.rust_reverse_unavailable_digest += counters.reverse_unavailable.digest;
    summary.rust_reverse_unavailable_malformed += counters.reverse_unavailable.malformed;
    summary.rust_reverse_unavailable_missing_record +=
        counters.reverse_unavailable.missing_record;
    if counters.reverse_published {
        summary.rust_reverse_published = true;
    }
    summary.rust_reverse_snapshots_reclaimed += counters.reverse_snapshots_reclaimed;
}
