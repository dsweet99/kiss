mod kernel_rules;
pub(crate) mod records;
mod runtime;
mod stored;
mod timing;
mod witness;
mod witness_reuse;
mod witness_summary;

pub(crate) use kernel_rules::{AllModePlan, KernelRules, emit_kernel_stage};
#[allow(unused_imports)]
pub(crate) use runtime::{
    EnsureRequest, EnsureRuntimeResult, LanguageEnsureResult, LanguageRuntime, Listing,
    OutcomeBatch,
};
pub(crate) use stored::GenerationIds;
pub(crate) use timing::timing_context_is_comparable;
pub(crate) use witness::{
    AcceptDecision, AcceptMode, ExecutionWitness, WitnessStatus, accept_witness,
    all_misses_warm_skippable, miss_selectors_for_repair, reclassify_statuses_with_gate,
    union_force_selectors_into_misses,
};
pub(crate) use witness_summary::summary_from_witness_statuses;

#[cfg(test)]
#[path = "witness_test.rs"]
mod witness_test;

#[cfg(test)]
#[path = "runtime_layout_test.rs"]
mod runtime_layout_test;
