use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use rpytest_runner::TestStatus;
use rust_llvm_cov_runner::RustLineCoverage;

use crate::test_runner::coverage_decision::{CoverageFreshness, RustSelectionBasis};
use crate::test_runner::rust_coverage_index::{
    current_rust_coverage_batch_identity, rebuild_rust_coverage_index, ResolvedRustPopulation,
    resolve_rust_population_state, select_rust_source_selectors_for_basis,
    write_rust_population_manifest_for_args, write_test_entry,
};

#[test]
fn resolve_reusable_prior_population_after_ordinary_source_edit() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(
        &lib,
        "pub fn value() -> u32 { 1 }\n#[cfg(test)] mod tests { #[test] fn gets_value() { assert_eq!(super::value(), 1); } }\n",
    )
    .unwrap();
    let _ = current_rust_coverage_batch_identity(tmp.path(), &[]);
    write_test_entry(
        tmp.path(),
        "value",
        "tests::gets_value",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(
                "src/lib.rs".to_string(),
                BTreeSet::from([1]),
            )]),
        },
    );
    rebuild_rust_coverage_index(tmp.path()).unwrap();
    write_rust_population_manifest_for_args(tmp.path(), &["tests::gets_value".to_string()], &[])
        .unwrap();

    fs::write(
        &lib,
        "pub fn value() -> u32 { 2 }\n#[cfg(test)] mod tests { #[test] fn gets_value() { assert_eq!(super::value(), 2); } }\n",
    )
    .unwrap();

    let resolved: ResolvedRustPopulation = resolve_rust_population_state(
        tmp.path(),
        &[],
        std::slice::from_ref(&lib),
        &[],
    )
    .expect("resolved population");
    assert_eq!(resolved.freshness, CoverageFreshness::ReusablePrior);
    assert_eq!(resolved.basis, RustSelectionBasis::ReusablePrior);
    let selected = select_rust_source_selectors_for_basis(
        tmp.path(),
        std::slice::from_ref(&lib),
        &BTreeMap::new(),
        &[],
        &resolved,
    )
    .expect("reusable-prior selectors");
    assert_eq!(selected, BTreeSet::from(["tests::gets_value".to_string()]));
}

#[test]
fn reusable_prior_selection_fails_on_missing_index_row() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    let missing = tmp.path().join("src").join("missing.rs");
    fs::write(&lib, "pub fn value() -> u32 { 1 }\n").unwrap();
    fs::write(&missing, "pub fn missing() -> u32 { 1 }\n").unwrap();
    let population = rust_llvm_cov_runner::RustPopulationState {
        input_fingerprint: String::new(),
        generation_fingerprint: String::new(),
        selection_context_fingerprint: String::new(),
        entries_fingerprint: String::new(),
        selectors: Vec::new(),
        line_index: BTreeMap::from([(
            "src/lib.rs".to_string(),
            BTreeSet::from(["tests::gets_value".to_string()]),
        )]),
    };
    let resolved = ResolvedRustPopulation {
        freshness: CoverageFreshness::ReusablePrior,
        basis: RustSelectionBasis::ReusablePrior,
        state: Some(population),
    };
    assert!(
        select_rust_source_selectors_for_basis(
            tmp.path(),
            &[lib, missing],
            &BTreeMap::new(),
            &[],
            &resolved,
        )
        .is_none()
    );
}

#[test]
fn reusable_prior_selection_fails_on_empty_index_row() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn value() -> u32 { 1 }\n").unwrap();
    let population = rust_llvm_cov_runner::RustPopulationState {
        input_fingerprint: String::new(),
        generation_fingerprint: String::new(),
        selection_context_fingerprint: String::new(),
        entries_fingerprint: String::new(),
        selectors: Vec::new(),
        line_index: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::new())]),
    };
    let resolved = ResolvedRustPopulation {
        freshness: CoverageFreshness::ReusablePrior,
        basis: RustSelectionBasis::ReusablePrior,
        state: Some(population),
    };
    assert!(
        select_rust_source_selectors_for_basis(
            tmp.path(),
            std::slice::from_ref(&lib),
            &BTreeMap::new(),
            &[],
            &resolved,
        )
        .is_none()
    );
}

#[test]
fn reusable_prior_selects_all_file_level_selectors_for_affected_file() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn value() -> u32 { 1 }\n").unwrap();
    let population = rust_llvm_cov_runner::RustPopulationState {
        input_fingerprint: String::new(),
        generation_fingerprint: String::new(),
        selection_context_fingerprint: String::new(),
        entries_fingerprint: String::new(),
        selectors: Vec::new(),
        line_index: BTreeMap::from([(
            "src/lib.rs".to_string(),
            BTreeSet::from([
                "tests::covers_line_one".to_string(),
                "tests::covers_line_two".to_string(),
            ]),
        )]),
    };
    let resolved = ResolvedRustPopulation {
        freshness: CoverageFreshness::ReusablePrior,
        basis: RustSelectionBasis::ReusablePrior,
        state: Some(population),
    };
    let selected = select_rust_source_selectors_for_basis(
        tmp.path(),
        std::slice::from_ref(&lib),
        &BTreeMap::new(),
        &[],
        &resolved,
    )
    .expect("reusable-prior selectors");
    assert_eq!(
        selected,
        BTreeSet::from([
            "tests::covers_line_one".to_string(),
            "tests::covers_line_two".to_string(),
        ])
    );
}

#[test]
fn reusable_prior_unions_selectors_from_multiple_affected_files() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let a = tmp.path().join("src").join("a.rs");
    let b = tmp.path().join("src").join("b.rs");
    fs::write(&a, "pub fn a() {}\n").unwrap();
    fs::write(&b, "pub fn b() {}\n").unwrap();
    let population = rust_llvm_cov_runner::RustPopulationState {
        input_fingerprint: String::new(),
        generation_fingerprint: String::new(),
        selection_context_fingerprint: String::new(),
        entries_fingerprint: String::new(),
        selectors: Vec::new(),
        line_index: BTreeMap::from([
            (
                "src/a.rs".to_string(),
                BTreeSet::from(["tests::covers_a".to_string()]),
            ),
            (
                "src/b.rs".to_string(),
                BTreeSet::from(["tests::covers_b".to_string()]),
            ),
        ]),
    };
    let resolved = ResolvedRustPopulation {
        freshness: CoverageFreshness::ReusablePrior,
        basis: RustSelectionBasis::ReusablePrior,
        state: Some(population),
    };
    let selected = select_rust_source_selectors_for_basis(
        tmp.path(),
        &[a, b],
        &BTreeMap::new(),
        &[],
        &resolved,
    )
    .expect("reusable-prior selectors");
    assert_eq!(
        selected,
        BTreeSet::from(["tests::covers_a".to_string(), "tests::covers_b".to_string()])
    );
}

#[test]
fn reusable_prior_uses_file_level_selectors_without_line_coordinate_matching() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn value() -> u32 { 1 }\n").unwrap();
    let population = rust_llvm_cov_runner::RustPopulationState {
        input_fingerprint: String::new(),
        generation_fingerprint: String::new(),
        selection_context_fingerprint: String::new(),
        entries_fingerprint: String::new(),
        selectors: Vec::new(),
        line_index: BTreeMap::from([(
            "src/lib.rs".to_string(),
            BTreeSet::from(["tests::covers_old_line".to_string()]),
        )]),
    };
    let resolved = ResolvedRustPopulation {
        freshness: CoverageFreshness::ReusablePrior,
        basis: RustSelectionBasis::ReusablePrior,
        state: Some(population),
    };
    let changed_lines = BTreeMap::from([(lib.clone(), BTreeSet::from([99u32]))]);
    let selected = select_rust_source_selectors_for_basis(
        tmp.path(),
        std::slice::from_ref(&lib),
        &changed_lines,
        &[],
        &resolved,
    )
    .expect("reusable-prior ignores changed-line coordinates");
    assert_eq!(selected, BTreeSet::from(["tests::covers_old_line".to_string()]));
}

#[test]
fn reusable_prior_selection_fails_on_non_rust_extension() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let txt = tmp.path().join("src").join("notes.txt");
    fs::write(&txt, "not rust\n").unwrap();
    let population = rust_llvm_cov_runner::RustPopulationState {
        input_fingerprint: String::new(),
        generation_fingerprint: String::new(),
        selection_context_fingerprint: String::new(),
        entries_fingerprint: String::new(),
        selectors: Vec::new(),
        line_index: BTreeMap::new(),
    };
    let resolved = ResolvedRustPopulation {
        freshness: CoverageFreshness::ReusablePrior,
        basis: RustSelectionBasis::ReusablePrior,
        state: Some(population),
    };
    assert!(
        select_rust_source_selectors_for_basis(
            tmp.path(),
            std::slice::from_ref(&txt),
            &BTreeMap::new(),
            &[],
            &resolved,
        )
        .is_none()
    );
}

#[test]
fn resolve_stale_population_when_no_manifest_matches() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn value() -> u32 { 1 }\n").unwrap();
    let resolved: ResolvedRustPopulation = resolve_rust_population_state(
        tmp.path(),
        &[],
        std::slice::from_ref(&lib),
        &[],
    )
    .expect("resolved population");
    assert_eq!(resolved.freshness, CoverageFreshness::Stale);
    assert_eq!(resolved.basis, RustSelectionBasis::Population);
    assert!(resolved.state.is_none());
}

#[test]
fn reusable_prior_is_not_claimed_complete_behavioral_impact_analysis() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let x = tmp.path().join("src").join("x.rs");
    let y = tmp.path().join("src").join("y.rs");
    fs::write(&x, "pub fn x() {}\n").unwrap();
    fs::write(&y, "pub fn y() {}\n").unwrap();
    let population = rust_llvm_cov_runner::RustPopulationState {
        input_fingerprint: String::new(),
        generation_fingerprint: String::new(),
        selection_context_fingerprint: String::new(),
        entries_fingerprint: String::new(),
        selectors: Vec::new(),
        line_index: BTreeMap::from([(
            "src/x.rs".to_string(),
            BTreeSet::from(["tests::covers_x_only".to_string()]),
        )]),
    };
    let resolved = ResolvedRustPopulation {
        freshness: CoverageFreshness::ReusablePrior,
        basis: RustSelectionBasis::ReusablePrior,
        state: Some(population),
    };
    let selected = select_rust_source_selectors_for_basis(
        tmp.path(),
        std::slice::from_ref(&y),
        &BTreeMap::new(),
        &[],
        &resolved,
    );
    assert!(
        selected.is_none(),
        "prior LLVM attribution for y.rs is absent, so reusable-prior selection fails closed"
    );
    let selected_x = select_rust_source_selectors_for_basis(
        tmp.path(),
        std::slice::from_ref(&x),
        &BTreeMap::new(),
        &[],
        &resolved,
    )
    .expect("prior attribution exists for x.rs");
    assert_eq!(selected_x, BTreeSet::from(["tests::covers_x_only".to_string()]));
}

#[test]
fn added_deleted_and_missing_rust_paths_require_population() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    let added = tmp.path().join("src").join("added.rs");
    let deleted = tmp.path().join("src").join("removed.rs");
    fs::write(&lib, "pub fn value() -> u32 { 1 }\n").unwrap();
    fs::write(&added, "pub fn added() -> u32 { 1 }\n").unwrap();
    let population = rust_llvm_cov_runner::RustPopulationState {
        input_fingerprint: String::new(),
        generation_fingerprint: String::new(),
        selection_context_fingerprint: String::new(),
        entries_fingerprint: String::new(),
        selectors: Vec::new(),
        line_index: BTreeMap::from([(
            "src/lib.rs".to_string(),
            BTreeSet::from(["tests::gets_value".to_string()]),
        )]),
    };
    let resolved = ResolvedRustPopulation {
        freshness: CoverageFreshness::ReusablePrior,
        basis: RustSelectionBasis::ReusablePrior,
        state: Some(population),
    };
    assert!(
        select_rust_source_selectors_for_basis(
            tmp.path(),
            std::slice::from_ref(&added),
            &BTreeMap::new(),
            &[],
            &resolved,
        )
        .is_none(),
        "added path has no prior non-empty row"
    );
    assert!(
        select_rust_source_selectors_for_basis(
            tmp.path(),
            std::slice::from_ref(&deleted),
            &BTreeMap::new(),
            &[],
            &resolved,
        )
        .is_none(),
        "deleted/missing path has no prior non-empty row"
    );
    let selected_lib = select_rust_source_selectors_for_basis(
        tmp.path(),
        std::slice::from_ref(&lib),
        &BTreeMap::new(),
        &[],
        &resolved,
    )
    .expect("ordinary covered path remains selectable");
    assert_eq!(
        selected_lib,
        BTreeSet::from(["tests::gets_value".to_string()])
    );
}
