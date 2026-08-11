//! Rust language runtime, witness store, and llvm-cov adapters.

mod witness_store;
mod runtime;
mod publish_merge;
pub(crate) mod llvm_cov;
pub(crate) mod workspace;
pub(crate) mod backer;

pub(crate) use witness_store::{
    PublishRustWitness, RustWarmDecision, maybe_bootstrap_rust_witness,
    publish_rust_execution_witness, rust_identity_digest_from_batch, rust_warm_or_miss_selectors,
    try_load_rust_execution_witness, try_warm_rust_cached_summary,
};
#[cfg(test)]
pub(crate) use witness_store::rust_miss_selectors;

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
