use crate::test_runner::coverage_decision::{CoverageFreshness, RustSelectionBasis};
use crate::test_runner::rust_coverage_index::{
    ResolvedRustPopulation, current_rust_coverage_batch_identity,
    select_rust_source_selectors_for_basis, write_test_entry,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

fn write_line_entry(repo_root: &Path, lib: &Path, name: &str, line: u32) {
    write_test_entry(
        repo_root,
        name,
        &format!("tests::{name}"),
        rpytest_runner::TestStatus::Passed,
        rust_llvm_cov_runner::RustLineCoverage {
            files: BTreeMap::from([(lib.to_string_lossy().to_string(), BTreeSet::from([line]))]),
        },
    );
}

fn reusable_prior_state(repo_root: &Path) -> ResolvedRustPopulation {
    let identity = current_rust_coverage_batch_identity(repo_root, &[]).unwrap();
    let selector_names = ["tests::alpha".to_string(), "tests::beta".to_string()];
    ResolvedRustPopulation {
        freshness: CoverageFreshness::ReusablePrior,
        basis: RustSelectionBasis::ReusablePrior,
        state: Some(rust_llvm_cov_runner::RustPopulationState {
            input_fingerprint: String::new(),
            generation_fingerprint: identity.generation_fingerprint,
            selection_context_fingerprint: String::new(),
            entries_fingerprint: String::new(),
            selectors: selector_names.to_vec(),
            line_index: BTreeMap::from([(
                "src/lib.rs".to_string(),
                BTreeSet::from(selector_names),
            )]),
            ordinary_source_digests: BTreeMap::new(),
            test_binaries: BTreeMap::new(),
        }),
        snapshot_delta: None,
    }
}

#[test]
fn reusable_prior_uses_selector_entries_for_changed_line_precision() {
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
        "pub fn alpha() -> u32 {\n    1\n}\n\npub fn beta() -> u32 {\n    2\n}\n",
    )
    .unwrap();
    write_line_entry(tmp.path(), &lib, "alpha", 2);
    write_line_entry(tmp.path(), &lib, "beta", 6);

    let changed_lines = BTreeMap::from([(lib.clone(), BTreeSet::from([2u32]))]);
    let selected = select_rust_source_selectors_for_basis(
        tmp.path(),
        std::slice::from_ref(&lib),
        &changed_lines,
        &[],
        &reusable_prior_state(tmp.path()),
    )
    .expect("line-precise reusable-prior selectors");

    assert_eq!(selected, BTreeSet::from(["tests::alpha".to_string()]));
}
