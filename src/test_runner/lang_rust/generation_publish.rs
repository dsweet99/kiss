use std::collections::BTreeMap;
use std::path::Path;

use crate::test_runner::execution_generation::{
    FullExecutionGeneration, SelectorEvidenceRecord, load_current_generation,
    publish_full_generation, reclaim_unreferenced,
};
use crate::test_runner::lang_iface::{ExecutionWitness, WitnessScope, WitnessStatus};
use crate::test_runner::rust_coverage_index::rust_coverage_cache_root;

pub(super) fn publish_complete_full_generation(
    repo_root: &Path,
    identity_digest: &str,
    selectors: &[String],
    statuses: &[WitnessStatus],
    durations_ns: &[Option<u64>],
    timing_context_digest: &str,
    covered_lines: &BTreeMap<String, Vec<u32>>,
) -> Result<String, String> {
    let cache_root = rust_coverage_cache_root(repo_root);
    let generation_id = publish_full_generation(
        &cache_root,
        rust_generation(
            identity_digest,
            selectors,
            statuses,
            durations_ns,
            timing_context_digest,
            covered_lines,
            true,
        ),
    )?;
    let _ = reclaim_unreferenced(&cache_root);
    Ok(generation_id)
}

pub(super) struct WitnessGenerationState<'a> {
    pub(super) timing_context_digest: &'a str,
    pub(super) complete: bool,
}

pub(super) fn publish_current_witness_generation(
    repo_root: &Path,
    identity_digest: &str,
    selectors: &[String],
    statuses: &[WitnessStatus],
    durations_ns: &[Option<u64>],
    covered_lines: &BTreeMap<String, Vec<u32>>,
    state: WitnessGenerationState<'_>,
) -> Result<String, String> {
    let cache_root = rust_coverage_cache_root(repo_root);
    let generation_id = crate::test_runner::execution_generation::publish_witness_generation(
        &cache_root,
        rust_generation(
            identity_digest,
            selectors,
            statuses,
            durations_ns,
            state.timing_context_digest,
            covered_lines,
            state.complete,
        ),
    )?;
    let _ = reclaim_unreferenced(&cache_root);
    Ok(generation_id)
}

pub(super) fn load_full_generation_witness(repo_root: &Path) -> Result<ExecutionWitness, String> {
    let cache_root = rust_coverage_cache_root(repo_root);
    let (generation, _pin) = load_current_generation(&cache_root)?;
    witness_from_generation(generation)
        .ok_or_else(|| "error: kiss: malformed Rust witness generation".to_string())
}

#[cfg(test)]
pub(super) fn try_load_full_generation_witness(repo_root: &Path) -> Option<ExecutionWitness> {
    load_full_generation_witness(repo_root).ok()
}

fn rust_generation(
    identity_digest: &str,
    selectors: &[String],
    statuses: &[WitnessStatus],
    durations_ns: &[Option<u64>],
    timing_context_digest: &str,
    covered_lines: &BTreeMap<String, Vec<u32>>,
    complete: bool,
) -> FullExecutionGeneration {
    let mut selector_evidence = Vec::with_capacity(selectors.len());
    let mut all_pass = complete;
    for ((selector, status), duration) in selectors
        .iter()
        .zip(statuses.iter())
        .zip(durations_ns.iter())
    {
        let raw_status = status.as_str();
        if *status != WitnessStatus::Passed {
            all_pass = false;
        }
        selector_evidence.push(SelectorEvidenceRecord {
            selector: selector.clone(),
            raw_status: raw_status.to_string(),
            duration_ns: *duration,
            entry_content_digest: crate::test_runner::execution_generation::sha256_hex(
                format!("{identity_digest}:{selector}:{raw_status}:{duration:?}").as_bytes(),
            ),
            evidence_state: "valid".into(),
            timing_context_digest: timing_context_digest.to_string(),
            ..Default::default()
        });
    }
    FullExecutionGeneration {
        schema_version: crate::test_runner::execution_generation::GENERATION_SCHEMA_VERSION
            .to_string(),
        generation_id: String::new(),
        execution_context_digest: identity_digest.to_string(),
        discovered_universe_digest: crate::test_runner::execution_generation::sha256_hex(
            selectors.join("\n").as_bytes(),
        ),
        selectors: selectors.to_vec(),
        selector_evidence,
        functional_summary_all_pass: all_pass,
        content_digest: String::new(),
        timing_context_digest: timing_context_digest.to_string(),
        covered_lines: covered_lines.clone(),
        ..Default::default()
    }
}

fn witness_from_generation(generation: FullExecutionGeneration) -> Option<ExecutionWitness> {
    if generation.selectors.len() != generation.selector_evidence.len() {
        return None;
    }
    let raw_statuses = generation
        .selector_evidence
        .iter()
        .map(|record| WitnessStatus::parse(&record.raw_status))
        .collect();
    let statuses = generation
        .selector_evidence
        .iter()
        .map(|record| {
            if record.evidence_state == "conflict" {
                WitnessStatus::Failed
            } else {
                WitnessStatus::parse(&record.raw_status)
            }
        })
        .collect();
    let durations_ns = generation
        .selector_evidence
        .iter()
        .map(|record| record.duration_ns)
        .collect();
    let complete = generation.is_complete_all_pass();
    Some(ExecutionWitness {
        language: "rust".into(),
        scope: WitnessScope::Full,
        identity_digest: generation.execution_context_digest,
        selectors: generation.selectors,
        statuses,
        durations_ns,
        covered_lines: generation.covered_lines,
        complete,
        generation_id: generation.generation_id,
        raw_statuses,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_runner::lang_iface::WitnessStatus;

    #[test]
    fn publish_and_load_complete_generation() {
        let tmp = tempfile::tempdir().unwrap();
        let id = publish_complete_full_generation(
            tmp.path(),
            "ctx",
            &["a".into()],
            &[WitnessStatus::Passed],
            &[Some(1)],
            "timing",
            &BTreeMap::new(),
        )
        .unwrap();
        let loaded = try_load_full_generation_witness(tmp.path()).unwrap();
        assert_eq!(loaded.generation_id, id);
        assert_eq!(loaded.selectors, vec!["a".to_string()]);
        assert!(loaded.complete);
    }

    #[test]
    fn failed_selector_is_not_published_as_full() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(
            publish_complete_full_generation(
                tmp.path(),
                "ctx",
                &["a".into()],
                &[WitnessStatus::Failed],
                &[Some(1)],
                "timing",
                &BTreeMap::new(),
            )
            .is_err()
        );
        assert!(try_load_full_generation_witness(tmp.path()).is_none());
    }

    #[test]
    fn witness_from_generation_rejects_shape_mismatch() {
        let mut generation = rust_generation(
            "ctx",
            &["a".into()],
            &[WitnessStatus::Passed],
            &[Some(1)],
            "timing",
            &BTreeMap::new(),
            true,
        );
        generation.selectors.push("b".into());
        assert!(witness_from_generation(generation).is_none());
    }
}
