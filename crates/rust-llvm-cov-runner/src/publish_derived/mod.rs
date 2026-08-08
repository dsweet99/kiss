//! Internal `publish_derived` package for rust-llvm-cov-runner call flow.

pub(crate) mod batch_check_aggregate;

pub(crate) mod batch_check_aggregate_identity;

pub(crate) mod batch_derived;

pub(crate) mod batch_derived_entries;

pub(crate) mod batch_derived_generations;

pub(crate) mod batch_derived_incremental;

pub(crate) mod batch_derived_index;

pub(crate) mod batch_derived_index_check_aggregate_support;

pub(crate) mod batch_derived_index_reverse;

pub(crate) mod batch_derived_index_types;

pub(crate) mod batch_derived_index_write;

pub(crate) mod batch_derived_manifest;

pub(crate) mod batch_population_durations;

pub(crate) mod batch_derived_prune;

pub(crate) mod batch_entry_state;

pub(crate) mod batch_io_skip_not_found;

pub(crate) mod batch_reverse_build;

pub(crate) mod batch_reverse_publish;

pub(crate) mod batch_reverse_query;

pub(crate) mod batch_reverse_query_metrics;

pub(crate) mod batch_reverse_line_index;

#[cfg(test)]
pub(crate) mod batch_reverse_test_support;

#[cfg(test)]
pub(crate) mod batch_reverse_process_race_support;

pub(crate) mod batch_derived_snapshot;

pub(crate) mod batch_publication_tmp;

#[cfg(test)]
pub(crate) mod batch_derived_index_witness_test;
