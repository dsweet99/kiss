use std::fs;

use super::pin::acquire_reader_pin;
use super::{
    FullExecutionGeneration, SelectorEvidenceRecord, load_current_generation,
    publish_full_generation, reclaim_unreferenced,
};

fn passed_record(selector: &str) -> SelectorEvidenceRecord {
    SelectorEvidenceRecord {
        selector: selector.to_string(),
        raw_status: "passed".into(),
        duration_ns: Some(1),
        entry_content_digest: format!("blob-{selector}"),
        evidence_state: "valid".into(),
        ..Default::default()
    }
}

fn generation(selectors: &[&str]) -> FullExecutionGeneration {
    FullExecutionGeneration {
        schema_version: super::GENERATION_SCHEMA_VERSION.to_string(),
        generation_id: String::new(),
        execution_context_digest: "ctx".into(),
        discovered_universe_digest: "uni".into(),
        selectors: selectors.iter().map(|s| (*s).to_string()).collect(),
        selector_evidence: selectors.iter().map(|s| passed_record(s)).collect(),
        functional_summary_all_pass: true,
        content_digest: String::new(),
        ..Default::default()
    }
}

#[test]
fn identical_payloads_share_generation_id() {
    let tmp = tempfile::tempdir().unwrap();
    let first = publish_full_generation(tmp.path(), generation(&["a", "b"])).unwrap();
    let second = publish_full_generation(tmp.path(), generation(&["a", "b"])).unwrap();
    assert_eq!(first, second);
}

#[test]
fn repair_sets_parent_and_keeps_universe() {
    let tmp = tempfile::tempdir().unwrap();
    let parent = publish_full_generation(tmp.path(), generation(&["a", "b"])).unwrap();
    let mut repaired = generation(&["a", "b"]);
    repaired.selector_evidence[1].entry_content_digest = "blob-b-repaired".into();
    let child = publish_full_generation(tmp.path(), repaired).unwrap();
    assert_ne!(parent, child);
    let (loaded, _pin) = load_current_generation(tmp.path()).unwrap();
    assert_eq!(loaded.generation_id, child);
    assert_eq!(loaded.selectors, vec!["a".to_string(), "b".to_string()]);
    let disjoint = generation(&["a", "c"]);
    let merged = super::rebase::rebase_incoming_on_current(&loaded, &disjoint).unwrap();
    assert!(merged.selectors.iter().any(|s| s == "c"));
    assert!(merged.is_complete_all_pass());
    let mut covered = loaded.clone();
    covered.covered_lines =
        std::collections::BTreeMap::from([("src/lib.rs".to_string(), vec![1, 2, 3])]);
    assert!(
        super::rebase::rebase_incoming_on_current(&covered, &disjoint).is_err(),
        "current-only selector coverage cannot be represented by incoming aggregate lines"
    );
    let same_universe_empty = generation(&["a", "b"]);
    let merged = super::rebase::rebase_incoming_on_current(&covered, &same_universe_empty).unwrap();
    assert_eq!(merged.covered_lines, covered.covered_lines);
    let mut overlap = generation(&["a", "b"]);
    overlap.selector_evidence[1].entry_content_digest = "blob-b-other".into();
    let conflicted = super::rebase::rebase_incoming_on_current(&loaded, &overlap).unwrap();
    assert!(!conflicted.is_complete_all_pass());
    assert!(
        conflicted
            .selector_evidence
            .iter()
            .any(|record| record.selector == "b" && record.evidence_state == "conflict")
    );
    let mut later_pass = generation(&["a", "b"]);
    later_pass.selector_evidence[1].entry_content_digest = "blob-b-later-pass".into();
    let still = super::rebase::rebase_incoming_on_current(&conflicted, &later_pass).unwrap();
    assert!(
        still
            .selector_evidence
            .iter()
            .any(|record| record.selector == "b" && record.evidence_state == "conflict"),
        "one later same-input PASS must not clear a conflict"
    );
    let mut other_id = generation(&["a", "b"]);
    other_id.execution_context_digest = "other-ctx".into();
    assert!(super::rebase::rebase_incoming_on_current(&loaded, &other_id).is_err());
}

#[test]
fn incomplete_generation_does_not_move_pointer() {
    let tmp = tempfile::tempdir().unwrap();
    let parent = publish_full_generation(tmp.path(), generation(&["a"])).unwrap();
    let mut incomplete = generation(&["a", "c"]);
    incomplete.functional_summary_all_pass = false;
    incomplete.selector_evidence[1].raw_status = "failed".into();
    assert!(publish_full_generation(tmp.path(), incomplete).is_err());
    let (loaded, _pin) = load_current_generation(tmp.path()).unwrap();
    assert_eq!(loaded.generation_id, parent);
    assert_eq!(loaded.selectors, vec!["a".to_string()]);
}

#[test]
fn reader_pin_survives_gc() {
    let tmp = tempfile::tempdir().unwrap();
    let first = publish_full_generation(tmp.path(), generation(&["a"])).unwrap();
    let _second = publish_full_generation(tmp.path(), generation(&["a", "b"])).unwrap();
    let pin = acquire_reader_pin(tmp.path(), &first).unwrap();
    reclaim_unreferenced(tmp.path()).unwrap();
    assert!(
        tmp.path()
            .join("generations")
            .join(&first)
            .join("generation.json")
            .is_file()
    );
    drop(pin);
    let orphan = "pending-orphan";
    let pending = super::pin::acquire_pending_pin(tmp.path(), orphan).unwrap();
    fs::create_dir_all(tmp.path().join("generations").join(orphan)).unwrap();
    fs::write(
        tmp.path()
            .join("generations")
            .join(orphan)
            .join("generation.json"),
        "{}\n",
    )
    .unwrap();
    reclaim_unreferenced(tmp.path()).unwrap();
    assert!(
        tmp.path()
            .join("generations")
            .join(orphan)
            .join("generation.json")
            .is_file(),
        "pending pin must retain unpublished generation"
    );
    drop(pending);
}

#[test]
fn publish_writes_content_addressed_evidence_blobs() {
    let tmp = tempfile::tempdir().unwrap();
    let id = publish_full_generation(tmp.path(), generation(&["a"])).unwrap();
    let digest = passed_record("a").entry_content_digest;
    assert!(
        tmp.path()
            .join("generations")
            .join(id)
            .join("evidence")
            .join(format!("{digest}.json"))
            .is_file()
    );
}
