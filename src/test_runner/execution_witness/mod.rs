//! Shared execution witness: one warm authority for `kiss test` and `kiss cov`.
//!
//! Design resolutions (from plan CP review):
//! - One Full pointer per language is All-mode current; subset runs filter that
//!   Full witness and do not publish a competing pinned Subset pointer.
//! - Time limits are applied at accept from stored raw durations (not baked into
//!   identity).
//! - `--lang` is All-mode relative to that language's discovered universe.

mod accept;
mod python_view;
mod rust_store;

pub(crate) use accept::{ExecutionWitness, WitnessScope, WitnessStatus};
pub(crate) use python_view::try_warm_python_cached_summary;
pub(crate) use rust_store::{
    PublishRustWitness, RustWarmDecision, maybe_bootstrap_rust_witness,
    publish_rust_execution_witness, rust_identity_digest_from_batch, rust_warm_or_miss_selectors,
    try_load_rust_execution_witness, try_warm_rust_cached_summary,
};
#[cfg(test)]
pub(crate) use rust_store::rust_miss_selectors;

#[cfg(test)]
#[path = "accept_test.rs"]
mod accept_test;

#[cfg(test)]
#[path = "rust_store_test.rs"]
mod rust_store_test;

#[cfg(test)]
#[path = "python_view_test.rs"]
mod python_view_test;

#[cfg(test)]
#[path = "bootstrap_test.rs"]
mod bootstrap_test;
