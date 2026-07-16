use crate::test_runner::coverage_decision::{CoverageFreshness, RustSelectionBasis};
use crate::test_runner::rust_coverage_index::{
    current_rust_coverage_batch_identity, rebuild_rust_coverage_index,
    resolve_rust_population_state, select_rust_source_selectors_for_basis,
    write_rust_population_manifest_for_args, write_test_entry,
};
use rpytest_runner::TestStatus;
use rust_llvm_cov_runner::RustLineCoverage;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

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
            files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
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

    let resolved = resolve_rust_population_state(tmp.path(), &[], std::slice::from_ref(&lib), &[])
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
