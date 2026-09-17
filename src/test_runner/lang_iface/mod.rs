mod runtime;
mod source_delta;
mod timing;
mod witness;
mod witness_reuse;

#[allow(unused_imports)]
pub(crate) use runtime::{CoverageSnapshot, StatusTimingSnapshot};
pub(crate) use runtime::{
    EnsureRequest, EnsureRuntimeResult, LanguageEnsureResult, LanguageRuntime, OutcomeBatch,
    PublishBatch,
};
pub(crate) use source_delta::SourceDeltaMisses;
pub(crate) use timing::{session_timing_context_digest, timing_context_is_comparable};
pub(crate) use witness::{
    AcceptDecision, AcceptMode, ExecutionWitness, WitnessScope, WitnessStatus, accept_witness,
    all_misses_warm_skippable, identity_covers, miss_selectors_for_repair,
    prune_witness_to_known_selectors, reclassify_statuses_with_gate, summary_from_accepted_witness,
    summary_from_witness_statuses, union_force_selectors_into_misses,
};

#[cfg(test)]
#[path = "witness_test.rs"]
mod witness_test;

#[cfg(test)]
#[path = "witness_prune_test.rs"]
mod witness_prune_test;

#[cfg(test)]
#[path = "runtime_layout_test.rs"]
mod runtime_layout_test;
