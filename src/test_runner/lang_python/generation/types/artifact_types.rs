
use serde::{Deserialize, Serialize};

use super::identity_types::{
    CoveredLinesMap, LineIndexMap, PythonPopulationPlan, SelectorCoverageMap,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum GenerationReason {
    Complete,
    CompleteForce,
    SelectiveRepair,
    IncompleteRepair,
    ColdCov,
    Migration,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ArtifactDigest {
    pub(crate) name: String,
    pub(crate) byte_length: u64,
    pub(crate) sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GenerationManifest {
    pub(crate) schema_version: String,
    pub(crate) generation_id: String,
    pub(crate) plan: PythonPopulationPlan,
    pub(crate) complete: bool,
    pub(crate) artifacts: Vec<ArtifactDigest>,
    pub(crate) creation_reason: GenerationReason,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PopulationPointer {
    pub(crate) schema_version: String,
    pub(crate) generation_id: String,
    pub(crate) manifest_sha256: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TimingCacheDisposition {
    Hit,
    MissStored,
    MissUnstored,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SelectorTimingRecord {
    pub(crate) selector: String,
    pub(crate) raw_status: String,
    pub(crate) effective_status: String,
    pub(crate) duration_ns: Option<u64>,
    pub(crate) cache_disposition: TimingCacheDisposition,
    pub(crate) reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PinnedPythonGeneration {
    pub(crate) generation_id: String,
    pub(crate) plan: PythonPopulationPlan,
    pub(crate) complete: bool,
    pub(crate) coverage: CoveredLinesMap,
    pub(crate) timings: Vec<SelectorTimingRecord>,
    pub(crate) line_index: LineIndexMap,
    pub(crate) selector_coverage: SelectorCoverageMap,
}
