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
use crate::test_runner::{PlannedSelectors, SelectorRunOptions, plan_selectors, run_selectors};

fn git_in(dir: &Path) -> Command {
    crate::test_git::git_command(dir)
}

fn git_stdout(dir: &Path, args: &[&str]) -> String {
    let output = git_in(dir).args(args).output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
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

fn lib_source(value: u32) -> String {
    format!(
        "pub fn value() -> u32 {{ {value} }}\n#[cfg(test)]\nmod tests {{ #[test] fn gets_value() {{ assert_eq!(super::value(), {value}); }} }}\n"
    )
}

fn publish_lib_population(root: &Path) {
    let _ = current_rust_coverage_batch_identity(root, &[]);
    write_test_entry(
        root,
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
    rebuild_rust_coverage_index(root).unwrap();
    write_rust_population_manifest_for_args(root, &["tests::gets_value".to_string()], &[]).unwrap();
}

fn warm_base_demo_with_historical_source(tmp: &TempDir) -> (String, std::path::PathBuf) {
    init_git(tmp);
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, lib_source(1)).unwrap();
    git_in(tmp.path()).args(["add", "."]).status().unwrap();
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "baseline"])
            .status()
            .unwrap()
            .success()
    );
    let baseline = git_stdout(tmp.path(), &["rev-parse", "HEAD"]);
    fs::write(
        tmp.path().join("src").join("historical.rs"),
        "pub fn historical() -> u32 { 1 }\n",
    )
    .unwrap();
    git_in(tmp.path()).args(["add", "."]).status().unwrap();
    assert!(
        git_in(tmp.path())
            .args(["commit", "-m", "historical"])
            .status()
            .unwrap()
            .success()
    );
    publish_lib_population(tmp.path());
    fs::write(&lib, lib_source(2)).unwrap();
    (baseline, lib)
}

fn assert_base_delta_plan(planned: &PlannedSelectors, lib: &Path) {
    assert_eq!(
        planned.rust_selection_basis,
        RustSelectionBasis::ReusablePrior
    );
    assert!(!planned.rust_population_required);
    assert_eq!(planned.rust_source_paths, vec![lib.to_path_buf()]);
    assert_eq!(planned.rust_vcs_source_paths, 2);
    assert_eq!(planned.rust_snapshot_delta_modified, 1);
    assert!(!planned.rust_snapshot_delta_structural);
    assert_eq!(planned.rs_sel, vec!["tests::gets_value".to_string()]);
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

#[test]
fn plan_selectors_base_and_main_use_snapshot_delta_not_historical_sources() {
    let _cwd_guard = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    let (baseline, lib) = warm_base_demo_with_historical_source(&tmp);
    let orig = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let base_planned = plan_selectors(
        TestChangeMode::Base,
        None,
        Some(&baseline),
        &[],
        &[],
        Some(kiss::Language::Rust),
        None,
    )
    .expect("base plan selectors");
    let main_planned = plan_selectors(
        TestChangeMode::Main,
        Some(&baseline),
        None,
        &[],
        &[],
        Some(kiss::Language::Rust),
        None,
    )
    .expect("main plan selectors");
    std::env::set_current_dir(orig).unwrap();
    assert_base_delta_plan(&base_planned, &lib);
    assert_base_delta_plan(&main_planned, &lib);
}
