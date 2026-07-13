use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use rpytest_runner::TestStatus;
use rust_llvm_cov_runner::RustLineCoverage;
use tempfile::TempDir;

use crate::test_git::TestChangeMode;
use crate::test_runner::coverage_decision::RustSelectionBasis;
use crate::test_runner::runners::enumerate_workspace_rust_selectors;
use crate::test_runner::rust_coverage_index::{
    current_rust_coverage_batch_identity, rebuild_rust_coverage_index,
    write_rust_population_manifest_for_args, write_test_entry,
};
use crate::test_runner::{plan_selectors, run_selectors, PlannedSelectors, SelectorRunOptions};

fn git_in(dir: &Path) -> Command {
    crate::test_git::git_command(dir)
}

fn init_git(tmp: &TempDir) {
    assert!(git_in(tmp.path()).arg("init").status().unwrap().success());
    git_in(tmp.path())
        .args(["config", "user.email", "t@t.t"])
        .status()
        .unwrap();
    git_in(tmp.path())
        .args(["config", "user.name", "t"])
        .status()
        .unwrap();
}

fn warm_committed_demo(tmp: &TempDir) -> std::path::PathBuf {
    init_git(tmp);
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(
        &lib,
        "pub fn value() -> u32 { 1 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() { assert_eq!(super::value(), 1); } }\n",
    )
    .unwrap();
    let _ = current_rust_coverage_batch_identity(tmp.path(), &[]);
    write_test_entry(
        tmp.path(),
        "abc",
        "tests::gets_value",
        TestStatus::Passed,
        RustLineCoverage {
            files: BTreeMap::from([(
                "src/lib.rs".to_string(),
                std::collections::BTreeSet::from([1]),
            )]),
        },
    );
    rebuild_rust_coverage_index(tmp.path()).unwrap();
    write_rust_population_manifest_for_args(tmp.path(), &["tests::gets_value".to_string()], &[])
        .unwrap();
    git_in(tmp.path()).args(["add", "."]).status().unwrap();
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "warm"])
            .status()
            .unwrap()
            .success()
    );
    lib
}

#[test]
fn plan_selectors_commit_uses_reusable_prior_after_ordinary_rs_edit() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    let lib = warm_committed_demo(&tmp);
    fs::write(
        &lib,
        "pub fn value() -> u32 { 2 }\n#[cfg(test)]\nmod tests { #[test] fn gets_value() { assert_eq!(super::value(), 2); } }\n",
    )
    .unwrap();

    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let planned: PlannedSelectors = plan_selectors(
        TestChangeMode::Commit,
        None,
        None,
        &[],
        &[],
        Some(kiss::Language::Rust),
        None,
    )
    .expect("plan selectors");
    let universe = enumerate_workspace_rust_selectors(tmp.path(), &[]).unwrap();
    let code = run_selectors(
        &planned,
        SelectorRunOptions {
            dry_run: true,
            force_rerun: false,
            metrics: true,
            jobs: 1,
            extra: &[],
            plan_duration: std::time::Duration::ZERO,
        },
    )
    .unwrap();
    std::env::set_current_dir(orig).unwrap();

    assert_eq!(code, 0);
    assert!(!planned.rust_population_required);
    assert_eq!(
        planned.rust_selection_basis,
        RustSelectionBasis::ReusablePrior
    );
    assert_eq!(planned.rs_sel, vec!["tests::gets_value".to_string()]);
    assert!(planned.rs_sel.len() < universe.len() || universe.len() == 1);
    assert!(!planned.rs_sel.is_empty());
}
