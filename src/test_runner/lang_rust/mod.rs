pub(crate) mod backer;
mod bootstrap;
mod generation_publish;
pub(crate) mod llvm_cov;
mod publish_merge;
mod runtime;
mod witness_memo;
mod witness_store;
mod witness_warm;
pub(crate) mod workspace;

pub(crate) use bootstrap::maybe_bootstrap_rust_witness;
pub(crate) use witness_memo::try_recall_published_rust_covered_lines;
#[cfg(test)]
pub(crate) use witness_store::rust_miss_selectors;
pub(crate) use witness_store::{
    PublishRustWitness, publish_rust_execution_witness, rust_identity_digest_from_batch,
    try_load_rust_execution_witness, try_warm_rust_cached_summary,
};
pub(crate) use witness_warm::{
    RustWarmDecision, rust_source_delta_misses, rust_warm_or_miss_selectors,
};
#[cfg(test)]
pub(crate) use witness_warm::{apply_warm_invalidation, planned_misses_for};

pub(crate) use runtime::RustRuntime;

#[cfg(test)]
#[path = "witness_store_test.rs"]
mod witness_store_test;

#[cfg(test)]
#[path = "bootstrap_test.rs"]
mod bootstrap_test;

#[cfg(test)]
#[path = "publish_merge_test.rs"]
mod publish_merge_test;

#[cfg(test)]
#[path = "runtime_test.rs"]
mod runtime_test;
