use std::collections::BTreeMap;

use super::types::{FullExecutionGeneration, SelectorEvidenceRecord};

pub(super) fn rebase_incoming_on_current(
    current: &FullExecutionGeneration,
    incoming: &FullExecutionGeneration,
) -> Result<FullExecutionGeneration, String> {
    if current.execution_context_digest != incoming.execution_context_digest {
        return Err("error: kiss: stale generation writer identity moved".into());
    }
    Ok(merge_generations(current, incoming))
}

pub(super) fn merge_generations(
    current: &FullExecutionGeneration,
    incoming: &FullExecutionGeneration,
) -> FullExecutionGeneration {
    let mut by_sel: BTreeMap<String, SelectorEvidenceRecord> = BTreeMap::new();
    for record in &current.selector_evidence {
        by_sel.insert(record.selector.clone(), record.clone());
    }
    for record in &incoming.selector_evidence {
        match by_sel.get(&record.selector) {
            Some(prior) if prior.entry_content_digest != record.entry_content_digest => {
                let mut conflicted = record.clone();
                conflicted.evidence_state = "conflict".into();
                conflicted.raw_status = "failed".into();
                by_sel.insert(record.selector.clone(), conflicted);
            }
            None => {
                by_sel.insert(record.selector.clone(), record.clone());
            }
            Some(_) => {}
        }
    }
    let selectors: Vec<String> = by_sel.keys().cloned().collect();
    let selector_evidence: Vec<SelectorEvidenceRecord> = by_sel.into_values().collect();
    let all_pass = selector_evidence
        .iter()
        .all(|record| record.evidence_state == "valid" && record.raw_status == "passed");
    FullExecutionGeneration {
        schema_version: current.schema_version.clone(),
        generation_id: String::new(),
        execution_context_digest: current.execution_context_digest.clone(),
        discovered_universe_digest: incoming.discovered_universe_digest.clone(),
        selectors,
        selector_evidence,
        functional_summary_all_pass: all_pass,
        content_digest: String::new(),
        source_snapshot_digest: current.source_snapshot_digest.clone(),
        test_definition_inventory_digest: current.test_definition_inventory_digest.clone(),
        timing_context_digest: current.timing_context_digest.clone(),
        coverage_index_digest: current.coverage_index_digest.clone(),
        reverse_index_digest: current.reverse_index_digest.clone(),
        binary_index_digest: current.binary_index_digest.clone(),
        covered_lines: incoming.covered_lines.clone(),
    }
}
