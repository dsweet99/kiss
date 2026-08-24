mod runtime;
mod witness;

#[allow(unused_imports)]
pub(crate) use runtime::{CoverageSnapshot, StatusTimingSnapshot};
pub(crate) use runtime::{
    EnsureRequest, EnsureRuntimeResult, LanguageEnsureResult, LanguageRuntime, OutcomeBatch,
    PublishBatch,
};
pub(crate) use witness::{
    AcceptDecision, AcceptMode, ExecutionWitness, WitnessScope, WitnessStatus, accept_witness,
    all_misses_warm_skippable, miss_selectors_for_repair, prune_witness_to_known_selectors,
    reclassify_statuses_with_gate, summary_from_accepted_witness, summary_from_witness_statuses,
    union_force_selectors_into_misses,
};

#[cfg(test)]
#[path = "witness_test.rs"]
mod witness_test;

#[cfg(test)]
#[path = "runtime_layout_test.rs"]
mod runtime_layout_test;
