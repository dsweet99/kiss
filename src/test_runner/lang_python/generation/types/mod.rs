//! Immutable Python population generation schemas.

mod identity_types;
mod artifact_types;

pub(crate) use artifact_types::{
    ArtifactDigest, GenerationManifest, GenerationReason, PinnedPythonGeneration,
    PopulationPointer, SelectorTimingRecord, TimingCacheDisposition,
};
pub(crate) use identity_types::{
    COLLECTOR_SEMANTICS_VERSION, CoveredLinesMap, GENERATION_SCHEMA_VERSION, LineIndexMap,
    POINTER_SCHEMA_VERSION, PythonExecutionIdentity, PythonPopulationPlan, RUNNER_SEMANTICS_VERSION,
    SelectorCoverageMap,
};
