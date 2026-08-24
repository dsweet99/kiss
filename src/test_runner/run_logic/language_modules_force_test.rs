use super::*;
use crate::test_runner::PlannedSelectors;
use kiss::rust_llvm_cov_runner::{
    RustCovCacheStatus, RustCoverageBatchCounters, RustLineCoverage, RustLlvmCovOutcome,
};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

fn planned() -> PlannedSelectors {
    let mut planned =
        crate::test_runner::test_mode_fixtures::empty_planned_selectors(PathBuf::from("."));
    planned.sel.rust = vec!["crate::tests::test_ok".to_string()];
    planned
}

#[test]
fn rust_prior_failure_does_not_set_batch_wide_force_rerun() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("src").join("lib.rs"),
        "#[cfg(test)]\nmod tests {\n    #[test]\n    fn alpha() {}\n}\n",
    )
    .unwrap();

    let selectors = vec!["tests::alpha".to_string()];
    let mut planned = planned();
    planned.repo_root = tmp.path().to_path_buf();
    planned.prior_failure_selectors.rust = vec!["tests::alpha".to_string()];
    let mut options = super::dry_run_selector_options();
    options.dry_run = false;
    options.force_rerun = false;
    options.jobs = 1;
    let ctx = crate::test_runner::coverage_decision::RunContext {
        planned: &planned,
        options: &options,
    };

    let force_seen = Rc::new(Cell::new(true));
    let force_seen_c = Rc::clone(&force_seen);
    let summary = run_rust_population_selectors_with_batch_deps(
        &selectors,
        &ctx,
        selectors.clone(),
        |_repo_root| {
            Ok(
                crate::test_runner::rust_llvm_cov::RustCoverageToolVersions {
                    cargo: "cargo 1.88.0".to_string(),
                    llvm_cov: "cargo-llvm-cov 0.6.0".to_string(),
                    rustc: "rustc 1.88.0".to_string(),
                    cargo_nextest: "cargo-nextest 0.9.0".to_string(),
                },
            )
        },
        move |batch_req, _versions| {
            force_seen_c.set(batch_req.force_rerun);
            Ok(kiss::rust_llvm_cov_runner::RustCoverageBatchResult {
                completed: batch_req
                    .logical_selectors
                    .iter()
                    .map(|selector| RustLlvmCovOutcome {
                        selector: selector.clone(),
                        status: kiss::rpytest_runner::TestStatus::Passed,
                        exit_code: Some(0),
                        duration: Duration::from_millis(1),
                        coverage: RustLineCoverage {
                            files: BTreeMap::new(),
                        },
                        test_binary_ids: vec!["test-bin".to_string()],
                        cache_status: RustCovCacheStatus::MissStored,
                        stdout: None,
                        stderr: None,
                    })
                    .collect(),
                batch_error: None,
                counters: RustCoverageBatchCounters::default(),
                test_binaries: Vec::new(),
            })
        },
    )
    .unwrap();

    assert!(!force_seen.get(), "prior Rust failure must not batch-force");
    assert_eq!(summary.total, 1);
}
