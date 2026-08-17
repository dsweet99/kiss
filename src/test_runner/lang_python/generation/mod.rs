//! Immutable Python coverage generations (v2).

mod types;
mod paths;
mod identity;
mod identity_memo;
mod evidence;
mod publish;
mod load;
mod memo;
mod materialize;
mod migrate;
mod repair;
mod current;
mod durations_load;

pub(crate) use current::{
    current_complete_generation_matches, current_generation_matches_plan,
    current_generation_plan_matches,
};
#[allow(unused_imports)]
pub(crate) use evidence::{PopulationEvidence, SelectorEvidence};
pub(crate) use identity::current_python_execution_identity;
pub(crate) use identity::identity_matches_current;
pub(crate) use identity_memo::clear_python_execution_identity_memo;
#[allow(unused_imports)]
pub(crate) use identity::population_plan_for_selectors;
#[allow(unused_imports)]
pub(crate) use load::{
    GenerationLoadError, generation_file_index, pinned_python_generation_artifacts_present,
    try_load_pinned_python_generation, try_load_pinned_python_generation_warm,
};
pub(crate) use materialize::{
    materialize_and_publish_from_cached_outcomes, selector_deltas_from_cached_outcomes,
};
pub(crate) use memo::clear_python_generation_warm_memo;
pub(crate) use migrate::try_migrate_complete_v1_generation;
#[allow(unused_imports)]
pub(crate) use publish::publish_python_population_generation;
pub(crate) use repair::{
    problem_selectors_from_timings, repair_python_population_generation,
};
#[allow(unused_imports)]
pub(crate) use types::{
    GenerationReason, PinnedPythonGeneration, POINTER_SCHEMA_VERSION, PythonExecutionIdentity,
    PythonPopulationPlan, SelectorTimingRecord, TimingCacheDisposition,
};
pub(crate) use durations_load::{
    try_load_generation_durations_pairs, try_load_generation_max_duration,
    try_load_generation_path_maxes_only,
};
pub(crate) use publish::{PathMaxDuration, path_maxes_from_selector_durations};

#[cfg(test)]
#[path = "publish_test.rs"]
mod publish_test;

#[cfg(test)]
#[path = "coverage_witness_test.rs"]
mod coverage_witness_test;

#[cfg(test)]
#[path = "identity_test.rs"]
mod identity_test;
