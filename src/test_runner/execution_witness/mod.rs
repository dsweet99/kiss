//! Shared execution witness: re-exports from language packages.
//!
//! Accept types live in `lang_iface`; Python view in `lang_python`; Rust store
//! in `lang_rust`. This module remains as a stable import path for call sites
//! not yet migrated to the language packages directly.

pub(crate) use crate::test_runner::lang_iface::{ExecutionWitness, WitnessScope, WitnessStatus};
#[allow(unused_imports)]
pub(crate) use crate::test_runner::lang_python::try_warm_python_cached_summary;
pub(crate) use crate::test_runner::lang_rust::{
    PublishRustWitness, RustWarmDecision, maybe_bootstrap_rust_witness,
    publish_rust_execution_witness, rust_identity_digest_from_batch, rust_warm_or_miss_selectors,
    try_load_rust_execution_witness, try_warm_rust_cached_summary,
};

/// Compatibility shim for callers that still import `execution_witness::accept`.
#[allow(unused_imports)]
pub(crate) mod accept {
    pub(crate) use crate::test_runner::lang_iface::{
        AcceptDecision, AcceptMode, ExecutionWitness, WitnessScope, WitnessStatus, accept_witness,
        miss_selectors_for_repair, reclassify_statuses_with_gate, summary_from_accepted_witness,
    };
}

#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use crate::test_runner::lang_rust::rust_miss_selectors;
