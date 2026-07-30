//! Generation-scoped derived reverse line index for fast PATH::symbol selection.
//!
//! Authoritative coverage remains per-test entry JSON. This index is disposable
//! and rebuildable from those entries. Readers trust only immutable snapshots
//! bound by population manifest + entry_state tokens.

pub use crate::batch_reverse_build::{REVERSE_LINE_INDEX_SCHEMA, ReversePublishInfo};
pub use crate::batch_reverse_publish::{
    prune_unreferenced_snapshots, publish_reverse_line_index, read_prior_snapshot_id,
    reverse_line_index_dir,
};

#[cfg(test)]
#[path = "batch_reverse_line_index_test.rs"]
mod tests;

#[cfg(test)]
#[path = "batch_reverse_query_contract_test.rs"]
mod query_contract_tests;

#[cfg(test)]
#[path = "batch_reverse_line_index_rebuild_test.rs"]
mod rebuild_tests;

#[cfg(test)]
#[path = "batch_reverse_prune_test.rs"]
mod prune_tests;

#[cfg(test)]
#[path = "batch_reverse_process_race_test.rs"]
mod process_race_tests;

#[cfg(test)]
#[path = "batch_reverse_process_race_b_test.rs"]
mod process_race_b_tests;
