use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub(crate) const GENERATION_SCHEMA_VERSION: &str = "kiss-execution-generation-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CurrentGenerationPointer {
    pub(crate) schema_version: String,
    pub(crate) generation_id: String,
    pub(crate) generation_manifest_digest: String,
    #[serde(default)]
    pub(crate) parent_generation_id: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SelectorEvidenceRecord {
    pub(crate) selector: String,
    pub(crate) raw_status: String,
    pub(crate) duration_ns: Option<u64>,
    pub(crate) entry_content_digest: String,
    pub(crate) evidence_state: String,
    #[serde(default)]
    pub(crate) test_definition_digest: String,
    #[serde(default)]
    pub(crate) timing_context_digest: String,
    #[serde(default)]
    pub(crate) cache_policy_digest: String,
    #[serde(default)]
    pub(crate) covered_lines_by_source: BTreeMap<String, Vec<u32>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FullExecutionGeneration {
    pub(crate) schema_version: String,
    pub(crate) generation_id: String,
    pub(crate) execution_context_digest: String,
    pub(crate) discovered_universe_digest: String,
    pub(crate) selectors: Vec<String>,
    pub(crate) selector_evidence: Vec<SelectorEvidenceRecord>,
    pub(crate) functional_summary_all_pass: bool,
    pub(crate) content_digest: String,
    #[serde(default)]
    pub(crate) source_snapshot_digest: String,
    #[serde(default)]
    pub(crate) test_definition_inventory_digest: String,
    #[serde(default)]
    pub(crate) timing_context_digest: String,
    #[serde(default)]
    pub(crate) coverage_index_digest: String,
    #[serde(default)]
    pub(crate) reverse_index_digest: String,
    #[serde(default)]
    pub(crate) binary_index_digest: String,
    #[serde(default)]
    pub(crate) covered_lines: BTreeMap<String, Vec<u32>>,
}

impl FullExecutionGeneration {
    pub(crate) fn semantic_payload(&self) -> FullExecutionGeneration {
        let mut payload = self.clone();
        payload.generation_id.clear();
        payload.content_digest.clear();
        payload
    }

    pub(crate) fn is_complete_all_pass(&self) -> bool {
        self.functional_summary_all_pass
            && self.selector_evidence.len() == self.selectors.len()
            && self
                .selector_evidence
                .iter()
                .all(|record| record.evidence_state == "valid" && record.raw_status == "passed")
    }
}
