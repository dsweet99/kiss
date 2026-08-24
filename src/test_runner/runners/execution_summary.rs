use std::time::Duration;

use kiss::rust_llvm_cov_runner::RustCoverageBatchCounters;

use super::merge_exit_codes;
use super::rust_batch_counters;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SelectorExecutionSummary {
    pub(crate) exit_code: i32,
    pub(crate) total: usize,
    pub(crate) cache_hits: usize,
    pub(crate) cache_misses: usize,
    pub(crate) cache_miss_selectors: Vec<String>,
    pub(crate) cache_unstored: usize,
    pub(crate) failed: usize,
    pub(crate) failed_selectors: Vec<String>,
    pub(crate) timed_out_selectors: Vec<String>,
    pub(crate) selector_durations_ns: std::collections::BTreeMap<String, u64>,
    pub(crate) raw_statuses: std::collections::BTreeMap<String, kiss::rpytest_runner::TestStatus>,
    pub(crate) max_passing_run_duration: Duration,
    pub(crate) rust_build_invocations: usize,
    pub(crate) rust_test_instances: usize,
    pub(crate) rust_export_jobs: usize,
    pub(crate) rust_aggregate_binaries: usize,
    pub(crate) rust_aggregate_exports: usize,
    pub(crate) rust_batch_cache_hits: usize,
    pub(crate) rust_max_active_test_instances: usize,
    pub(crate) rust_max_active_exports: usize,
    pub(crate) rust_unmatched_selectors: usize,
    pub(crate) rust_max_objects_per_export: usize,
    pub(crate) rust_build_target_baseline_bytes: u64,
    pub(crate) phase_rust_export_ms: u128,
    pub(crate) rust_derived_state_published: bool,
    pub(crate) rust_derived_repair: bool,
    pub(crate) rust_entry_generation_count: usize,
    pub(crate) rust_current_index_generation: String,
    pub(crate) rust_cache_pruned_entries: usize,
    pub(crate) rust_process_residual_count: usize,
    pub(crate) rust_legacy_cleanup_deferred: bool,
    pub(crate) rust_reverse_query_hits: u64,
    pub(crate) rust_reverse_unavailable_schema: u64,
    pub(crate) rust_reverse_unavailable_generation: u64,
    pub(crate) rust_reverse_unavailable_revision: u64,
    pub(crate) rust_reverse_unavailable_fingerprint: u64,
    pub(crate) rust_reverse_unavailable_digest: u64,
    pub(crate) rust_reverse_unavailable_malformed: u64,
    pub(crate) rust_reverse_unavailable_missing_record: u64,
    pub(crate) rust_reverse_published: bool,
    pub(crate) rust_reverse_snapshots_reclaimed: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SelectorCacheRecord {
    Hit,
    MissStored,
    MissUnstored,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SelectorExecutionRecord {
    pub(crate) selector: String,
    pub(crate) status: kiss::rpytest_runner::TestStatus,
    pub(crate) raw_status: Option<kiss::rpytest_runner::TestStatus>,
    pub(crate) cache_record: SelectorCacheRecord,
    pub(crate) exit_code: Option<i32>,
    pub(crate) duration: Duration,
}

impl SelectorExecutionSummary {
    pub(crate) fn record(&mut self, record: SelectorExecutionRecord) {
        self.total += 1;
        self.selector_durations_ns
            .insert(record.selector.clone(), record.duration.as_nanos() as u64);
        let raw = record.raw_status.unwrap_or(record.status);
        self.raw_statuses.insert(record.selector.clone(), raw);
        match record.cache_record {
            SelectorCacheRecord::Hit => self.cache_hits += 1,
            SelectorCacheRecord::MissStored => {
                self.cache_misses += 1;
                self.cache_miss_selectors.push(record.selector.clone());
            }
            SelectorCacheRecord::MissUnstored => {
                self.cache_misses += 1;
                self.cache_unstored += 1;
                self.cache_miss_selectors.push(record.selector.clone());
            }
        }
        match record.status {
            kiss::rpytest_runner::TestStatus::Failed => {
                self.failed += 1;
                self.failed_selectors.push(record.selector);
                self.exit_code = merge_exit_codes(self.exit_code, record.exit_code.unwrap_or(1));
            }
            kiss::rpytest_runner::TestStatus::TimedOut => {
                self.failed += 1;
                self.timed_out_selectors.push(record.selector);
                self.exit_code = merge_exit_codes(self.exit_code, record.exit_code.unwrap_or(1));
            }
            kiss::rpytest_runner::TestStatus::Passed => {
                if record.cache_record != SelectorCacheRecord::Hit {
                    self.max_passing_run_duration =
                        self.max_passing_run_duration.max(record.duration);
                }
            }
        }
    }

    pub(crate) fn record_rust_batch_counters(&mut self, counters: &RustCoverageBatchCounters) {
        self.rust_build_invocations += counters.build_invocations;
        self.rust_test_instances += counters.test_instances;
        self.rust_export_jobs += counters.export_jobs;
        self.rust_aggregate_binaries += counters.aggregate_binaries;
        self.rust_aggregate_exports += counters.aggregate_exports;
        self.rust_batch_cache_hits += counters.cache_hits;
        self.rust_max_active_test_instances = self
            .rust_max_active_test_instances
            .max(counters.max_active_test_instances);
        self.rust_max_active_exports = self
            .rust_max_active_exports
            .max(counters.max_active_exports);
        self.rust_unmatched_selectors += counters.unmatched_selectors;
        self.rust_max_objects_per_export = self
            .rust_max_objects_per_export
            .max(counters.max_objects_per_export);
        self.rust_build_target_baseline_bytes = self
            .rust_build_target_baseline_bytes
            .max(counters.build_target_baseline_bytes);
        self.phase_rust_export_ms += counters.export_phase_ms;
        if counters.derived_state_published {
            self.rust_derived_state_published = true;
        }
        if counters.derived_repair {
            self.rust_derived_repair = true;
        }
        self.rust_entry_generation_count = self
            .rust_entry_generation_count
            .max(counters.entry_generation_count);
        if !counters.current_index_generation.is_empty() {
            self.rust_current_index_generation = counters.current_index_generation.clone();
        }
        self.rust_cache_pruned_entries += counters.cache_pruned_entries;
        self.rust_process_residual_count += counters.process_residual_count;
        if counters.legacy_cleanup_deferred {
            self.rust_legacy_cleanup_deferred = true;
        }
        rust_batch_counters::record_reverse_batch_counters(self, counters);
    }
}

#[cfg(test)]
#[path = "execution_summary_test.rs"]
mod tests;
