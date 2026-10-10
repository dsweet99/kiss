mod kernel_hooks;
mod kernel_rules;
pub(crate) mod records;
mod runtime;
mod stored;
mod witness;
mod witness_reuse;
mod witness_summary;

pub(crate) use kernel_rules::{AllModePlan, KernelHooks, KernelRules};
#[allow(unused_imports)]
pub(crate) use runtime::{
    EnsureRequest, EnsureRuntimeResult, LanguageEnsureResult, LanguageRuntime, Listing,
    OutcomeBatch,
};
pub(crate) use stored::GenerationIds;
#[cfg(test)]
pub(crate) use witness::union_force_selectors_into_misses;
pub(crate) use witness::{
    AcceptDecision, AcceptMode, ExecutionWitness, WitnessStatus, accept_witness,
    miss_selectors_for_repair, reclassify_statuses_with_gate,
};
pub(crate) use witness_summary::summary_from_witness_statuses;

#[cfg(test)]
#[path = "witness_test.rs"]
mod witness_test;

#[cfg(test)]
#[path = "runtime_layout_test.rs"]
mod runtime_layout_test;
