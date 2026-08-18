use super::{
    ResolvedRustPopulation, current_partial_population_covers_selection,
    planned_check_aggregate_line_selectors, select_rust_source_selectors_for_basis,
};
use crate::test_runner::coverage_decision::{CoverageFreshness, SelectionBasis};
use rust_llvm_cov_runner::RustPopulationState;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn witness_resolved_population_enum() {
    let resolved = ResolvedRustPopulation::ReusablePrior {
        state: RustPopulationState {
            input_fingerprint: String::new(),
            generation_fingerprint: String::new(),
            selection_context_fingerprint: String::new(),
            entries_fingerprint: String::new(),
            selectors: Vec::new(),
            line_index: BTreeMap::new(),
            ordinary_source_digests: BTreeMap::new(),
            test_binaries: BTreeMap::new(),
        },
        delta: rust_llvm_cov_runner::RustSnapshotDelta::Unchanged,
    };
    assert_eq!(resolved.basis(), SelectionBasis::ReusablePrior);
    assert_eq!(resolved.freshness(), CoverageFreshness::ReusablePrior);
    assert!(resolved.state().is_some());
}

#[test]
fn partial_current_population_must_exactly_cover_changed_source_selection() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src").join("lib.rs");
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();
    std::fs::write(&src, "pub fn value() -> u32 { 1 }\n").unwrap();
    let population = RustPopulationState {
        input_fingerprint: "input".to_string(),
        generation_fingerprint: "generation".to_string(),
        selection_context_fingerprint: "selection".to_string(),
        entries_fingerprint: "entries".to_string(),
        selectors: vec!["tests::covers_src".to_string()],
        line_index: BTreeMap::from([(
            "src/lib.rs".to_string(),
            BTreeSet::from(["tests::covers_src".to_string()]),
        )]),
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: BTreeMap::new(),
    };

    assert!(current_partial_population_covers_selection(
        tmp.path(),
        std::slice::from_ref(&src),
        &BTreeMap::new(),
        &[],
        &population
    ));
    let mut extra_manifest_selector = population.clone();
    extra_manifest_selector
        .selectors
        .push("tests::not_selected".to_string());
    assert!(!current_partial_population_covers_selection(
        tmp.path(),
        &[src],
        &BTreeMap::new(),
        &[],
        &extra_manifest_selector
    ));
}

#[test]
fn check_aggregate_source_selection_returns_population_or_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src").join("lib.rs");
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();
    std::fs::write(&src, "pub fn value() -> u32 { 1 }\n").unwrap();

    let covered = RustPopulationState {
        input_fingerprint: "input".to_string(),
        generation_fingerprint: "generation".to_string(),
        selection_context_fingerprint: "selection".to_string(),
        entries_fingerprint: "check-aggregate:deadbeef".to_string(),
        selectors: vec!["tests::covers_src".to_string()],
        line_index: BTreeMap::from([(
            "src/lib.rs".to_string(),
            BTreeSet::new(), // compact check-aggregate: key presence only
        )]),
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: BTreeMap::new(),
    };
    let resolved = ResolvedRustPopulation::Current {
        state: covered.clone(),
    };
    let selected = select_rust_source_selectors_for_basis(
        tmp.path(),
        std::slice::from_ref(&src),
        &BTreeMap::new(),
        &[],
        &resolved,
    )
    .expect("selection");
    assert_eq!(
        selected,
        BTreeSet::from(["tests::covers_src".to_string()])
    );

    let uncovered = RustPopulationState {
        line_index: BTreeMap::new(),
        ..covered
    };
    let resolved = ResolvedRustPopulation::Current { state: uncovered };
    let selected = select_rust_source_selectors_for_basis(
        tmp.path(),
        &[src],
        &BTreeMap::new(),
        &[],
        &resolved,
    )
    .expect("selection");
    assert!(selected.is_empty());
}

#[test]
fn check_aggregate_line_selectors_drop_names_outside_the_planned_population() {
    let planned = vec!["tests::covers_src".to_string()];
    let selected = BTreeSet::from([
        "build_check_aggregate_reports_missing_binary_identity_and_line_map".to_string(),
        "tests::covers_src".to_string(),
    ]);
    assert_eq!(
        planned_check_aggregate_line_selectors(&selected, &planned),
        BTreeSet::from(["tests::covers_src".to_string()])
    );
    assert!(
        planned_check_aggregate_line_selectors(
            &BTreeSet::from(["nextest_only_name".to_string()]),
            &planned
        )
        .is_empty()
    );
}

#[test]
fn check_aggregate_rejects_non_rust_paths_on_reusable_prior() {
    let tmp = tempfile::tempdir().unwrap();
    let py = tmp.path().join("mod.py");
    std::fs::write(&py, "def x():\n    return 1\n").unwrap();
    let population = RustPopulationState {
        input_fingerprint: "input".to_string(),
        generation_fingerprint: "generation".to_string(),
        selection_context_fingerprint: "selection".to_string(),
        entries_fingerprint: "check-aggregate:deadbeef".to_string(),
        selectors: vec!["tests::covers_src".to_string()],
        line_index: BTreeMap::from([("mod.py".to_string(), BTreeSet::new())]),
        ordinary_source_digests: BTreeMap::new(),
        test_binaries: BTreeMap::new(),
    };
    let resolved = ResolvedRustPopulation::ReusablePrior {
        state: population,
        delta: rust_llvm_cov_runner::RustSnapshotDelta::Unchanged,
    };
    assert!(
        select_rust_source_selectors_for_basis(
            tmp.path(),
            &[py],
            &BTreeMap::new(),
            &[],
            &resolved,
        )
        .is_none()
    );
}
