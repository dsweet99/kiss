use super::{
    ResolveRustPopulationArgs, ResolvedRustPopulation, current_partial_population_covers_selection,
    planned_check_aggregate_line_selectors, resolve_rust_population_state,
    select_rust_source_selectors_for_basis,
};
use crate::test_runner::coverage_decision::{CoverageFreshness, SelectionBasis};
use kiss::rust_llvm_cov_runner::RustPopulationState;
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
        delta: kiss::rust_llvm_cov_runner::RustSnapshotDelta::Unchanged,
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
        &BTreeMap::from([(src.clone(), BTreeSet::from([1]))]),
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
        line_index: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::new())]),
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
    assert_eq!(selected, BTreeSet::from(["tests::covers_src".to_string()]));

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
        delta: kiss::rust_llvm_cov_runner::RustSnapshotDelta::Unchanged,
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

#[test]
fn resolve_exact_partial_and_reusable_loader_outcomes() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    let src = tmp.path().join("src").join("lib.rs");
    std::fs::write(
        &src,
        "pub fn value() -> u32 { 1 }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn gets_value() { assert_eq!(super::value(), 1); }\n}\n",
    )
    .unwrap();
    crate::test_runner::rust_coverage_index::write_test_entry(
        tmp.path(),
        "value",
        "tests::gets_value",
        kiss::rpytest_runner::TestStatus::Passed,
        kiss::rust_llvm_cov_runner::RustLineCoverage {
            files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
        },
    );
    crate::test_runner::rust_coverage_index::rebuild_rust_coverage_index(tmp.path()).unwrap();
    crate::test_runner::rust_coverage_index::write_rust_population_manifest_for_args(
        tmp.path(),
        &["tests::gets_value".to_string()],
        &[],
    )
    .unwrap();

    let exact_expected = ["tests::gets_value".to_string()];
    let exact = resolve_rust_population_state(ResolveRustPopulationArgs {
        repo_root: tmp.path(),
        ignore: &[],
        rust_source_paths: std::slice::from_ref(&src),
        rust_changed_lines: &BTreeMap::from([(src.clone(), BTreeSet::from([1]))]),
        expected_selectors: Some(&exact_expected),
        test_args: &[],
    })
    .expect("exact");
    assert!(matches!(exact, ResolvedRustPopulation::Current { .. }));

    let partial_expected = [
        "tests::gets_value".to_string(),
        "tests::new_selector".to_string(),
    ];
    let partial = resolve_rust_population_state(ResolveRustPopulationArgs {
        repo_root: tmp.path(),
        ignore: &[],
        rust_source_paths: std::slice::from_ref(&src),
        rust_changed_lines: &BTreeMap::from([(src.clone(), BTreeSet::from([1]))]),
        expected_selectors: Some(&partial_expected),
        test_args: &[],
    })
    .expect("partial");
    assert!(matches!(partial, ResolvedRustPopulation::Current { .. }));

    std::fs::write(
        &src,
        "pub fn value() -> u32 { 2 }\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn gets_value() { assert_eq!(super::value(), 2); }\n}\n",
    )
    .unwrap();
    let reusable = resolve_rust_population_state(ResolveRustPopulationArgs {
        repo_root: tmp.path(),
        ignore: &[],
        rust_source_paths: std::slice::from_ref(&src),
        rust_changed_lines: &BTreeMap::from([(src.clone(), BTreeSet::from([1]))]),
        expected_selectors: Some(&exact_expected),
        test_args: &[],
    })
    .expect("reusable");
    assert!(matches!(
        reusable,
        ResolvedRustPopulation::ReusablePrior { .. }
    ));
}
