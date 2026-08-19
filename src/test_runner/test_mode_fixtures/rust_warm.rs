use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use rpytest_runner::TestStatus;
use rust_llvm_cov_runner::RustLineCoverage;
use tempfile::TempDir;

use crate::test_runner::coverage_decision::SelectionBasis;
use crate::test_runner::rust_coverage_index::{
    rebuild_rust_coverage_index, write_rust_population_manifest_for_args, write_test_entry,
};
use crate::test_runner::PlannedSelectors;

use super::git::{checkout_branch, commit_all, ensure_main_branch, git_in, git_stdout, init_git};

pub(crate) const RS_COVERING_SELECTOR: &str = "tests::gets_value";

pub(crate) fn lib_source(value: u32) -> String {
    format!(
        "pub fn value() -> u32 {{ {value} }}\n#[cfg(test)]\nmod tests {{ #[test] fn gets_value() {{ assert_eq!(super::value(), {value}); }} }}\n"
    )
}

pub(crate) fn publish_lib_population(root: &Path) {
    write_test_entry(
        root,
        "abc",
        RS_COVERING_SELECTOR,
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(
                "src/lib.rs".to_string(),
                std::collections::BTreeSet::from([1]),
            )]),
        },
    );
    rebuild_rust_coverage_index(root).unwrap();
    write_rust_population_manifest_for_args(root, &[RS_COVERING_SELECTOR.to_string()], &[])
        .unwrap();
}

fn write_demo_crate(root: &Path, value: u32) -> PathBuf {
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();

    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let lib = root.join("src").join("lib.rs");
    fs::write(&lib, lib_source(value)).unwrap();
    lib
}

pub(crate) fn warm_committed_rust_demo(tmp: &TempDir) -> PathBuf {
    init_git(tmp);
    ensure_main_branch(tmp.path());
    let lib = write_demo_crate(tmp.path(), 1);
    publish_lib_population(tmp.path());
    commit_all(tmp.path(), "warm");
    lib
}

pub(crate) fn warm_multi_branch_rust_demo(tmp: &TempDir) -> PathBuf {
    let lib = warm_committed_rust_demo(tmp);
    checkout_branch(tmp.path(), "feature");
    lib
}

pub(crate) fn edit_rust_covered_source(lib: &Path, value: u32) {
    fs::write(lib, lib_source(value)).unwrap();
}

pub(crate) fn warm_base_demo_with_historical_source(tmp: &TempDir) -> (String, PathBuf) {
    init_git(tmp);
    ensure_main_branch(tmp.path());
    let lib = write_demo_crate(tmp.path(), 1);
    commit_all(tmp.path(), "baseline");
    let baseline = git_stdout(tmp.path(), &["rev-parse", "HEAD"]);
    fs::write(
        tmp.path().join("src").join("historical.rs"),
        "pub fn historical() -> u32 { 1 }\n",
    )
    .unwrap();
    assert!(
        git_in(tmp.path())
            .args(["add", "src/historical.rs"])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "historical"])
            .status()
            .unwrap()
            .success()
    );
    publish_lib_population(tmp.path());
    edit_rust_covered_source(&lib, 2);
    (baseline, lib)
}

pub(crate) fn assert_base_delta_plan(planned: &PlannedSelectors, lib: &Path) {
    assert_eq!(
        planned.selection_basis.rust,
        SelectionBasis::ReusablePrior
    );
    assert!(!planned.population_required.rust);
    assert_eq!(planned.source_paths.rust, vec![lib.to_path_buf()]);
    assert!(
        planned.vcs_source_paths.rust >= 2,
        "base/main: VCS range must include historical + edited paths, got {}",
        planned.vcs_source_paths.rust
    );
    assert_eq!(planned.snapshot_delta_modified.rust, 1);
    assert!(!planned.snapshot_delta_structural.rust);
    assert_eq!(planned.sel.rust, vec![RS_COVERING_SELECTOR.to_string()]);
}
