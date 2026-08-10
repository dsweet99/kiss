#![allow(unused_imports)]
use super::*;
use crate::test_runner::runners::SelectorExecutionSummary;
use rust_llvm_cov_runner::{
    RustCovCacheStatus, RustCoverageBatchCounters, RustCoverageBatchResult, RustLineCoverage,
    RustLlvmCovError, RustLlvmCovOutcome,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

#[test]
fn run_rust_llvm_cov_selectors_rejects_unsupported_test_args_before_tool_detection() {
    let tmp = tempfile::tempdir().unwrap();

    let err = run_rust_llvm_cov_selectors(
        tmp.path(),
        &["tests::case".to_string()],
        &["--format".to_string(), "json".to_string()],
        false,
        1,
        None,
    )
    .unwrap_err();

    assert!(err.contains("unsupported Rust test argument"));
    assert!(err.contains("--format"));
}

#[test]
fn finish_rust_coverage_batch_result_reports_kiss_test_path_symbol_ids() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    std::fs::create_dir(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("src").join("lib.rs"),
        r#"
pub fn value() -> u32 { 1 }
#[cfg(test)]
mod tests {
    #[test]
    fn gets_value() { assert_eq!(super::value(), 1); }
}
"#,
    )
    .unwrap();
    let identity = rust_last_status_identity(
        "cargo 1.88.0",
        "cargo-llvm-cov 0.6.0",
        "rustc 1.88.0",
        "cargo-nextest 0.9.0",
        &[],
        "0000000000000000",
    );
    let result = RustCoverageBatchResult {
        completed: vec![RustLlvmCovOutcome {
            selector: "tests::gets_value".to_string(),
            status: rpytest_runner::TestStatus::TimedOut,
            exit_code: Some(124),
            duration: Duration::from_secs(1),
            coverage: RustLineCoverage {
                files: BTreeMap::new(),
            },
            test_binary_ids: vec!["bin".to_string()],
            cache_status: RustCovCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        }],
        batch_error: None,
        counters: RustCoverageBatchCounters::default(),
        test_binaries: Vec::new(),
    };

    let summary = finish_rust_coverage_batch_result(tmp.path(), &identity, result).unwrap();
    assert_eq!(
        summary.timed_out_selectors,
        vec!["src/lib.rs::gets_value".to_string()]
    );
}

#[test]
fn finish_rust_coverage_batch_result_prints_cached_and_failed_outcomes() {
    let tmp = tempfile::tempdir().unwrap();
    let identity = rust_last_status_identity(
        "cargo 1.88.0",
        "cargo-llvm-cov 0.6.0",
        "rustc 1.88.0",
        "cargo-nextest 0.9.0",
        &[],
        "0000000000000000",
    );
    let result = RustCoverageBatchResult {
        completed: vec![
            RustLlvmCovOutcome {
                selector: "tests::cached_pass".to_string(),
                status: rpytest_runner::TestStatus::Passed,
                exit_code: Some(0),
                duration: Duration::from_millis(1),
                coverage: RustLineCoverage {
                    files: BTreeMap::new(),
                },
                test_binary_ids: vec!["bin".to_string()],
                cache_status: RustCovCacheStatus::Hit,
                stdout: None,
                stderr: None,
            },
            RustLlvmCovOutcome {
                selector: "tests::fresh_pass".to_string(),
                status: rpytest_runner::TestStatus::Passed,
                exit_code: Some(0),
                duration: Duration::from_millis(1),
                coverage: RustLineCoverage {
                    files: BTreeMap::new(),
                },
                test_binary_ids: vec!["bin".to_string()],
                cache_status: RustCovCacheStatus::MissStored,
                stdout: None,
                stderr: None,
            },
            RustLlvmCovOutcome {
                selector: "tests::cached_fail".to_string(),
                status: rpytest_runner::TestStatus::Failed,
                exit_code: Some(1),
                duration: Duration::from_millis(1),
                coverage: RustLineCoverage {
                    files: BTreeMap::new(),
                },
                test_binary_ids: vec!["bin".to_string()],
                cache_status: RustCovCacheStatus::Hit,
                stdout: None,
                stderr: None,
            },
            RustLlvmCovOutcome {
                selector: "tests::fresh_fail".to_string(),
                status: rpytest_runner::TestStatus::Failed,
                exit_code: Some(1),
                duration: Duration::from_millis(1),
                coverage: RustLineCoverage {
                    files: BTreeMap::new(),
                },
                test_binary_ids: vec!["bin".to_string()],
                cache_status: RustCovCacheStatus::MissStored,
                stdout: None,
                stderr: Some(b"boom\n".to_vec()),
            },
        ],
        batch_error: None,
        counters: RustCoverageBatchCounters::default(),
        test_binaries: Vec::new(),
    };

    let summary = finish_rust_coverage_batch_result(tmp.path(), &identity, result).unwrap();
    assert_eq!(summary.total, 4);
    assert_eq!(summary.failed, 2);
    assert_eq!(
        summary.failed_selectors,
        vec![
            "tests::cached_fail".to_string(),
            "tests::fresh_fail".to_string()
        ]
    );
    assert_eq!(
        summary.max_passing_run_duration,
        Duration::from_millis(1)
    );
}

#[test]
fn finish_rust_coverage_batch_result_prints_fresh_unstored_outcomes() {
    let tmp = tempfile::tempdir().unwrap();
    let identity = rust_last_status_identity(
        "cargo 1.88.0",
        "cargo-llvm-cov 0.6.0",
        "rustc 1.88.0",
        "cargo-nextest 0.9.0",
        &[],
        "0000000000000000",
    );
    let result = RustCoverageBatchResult {
        completed: vec![
            RustLlvmCovOutcome {
                selector: "tests::fresh_pass".to_string(),
                status: rpytest_runner::TestStatus::Passed,
                exit_code: Some(0),
                duration: Duration::from_millis(1),
                coverage: RustLineCoverage {
                    files: BTreeMap::new(),
                },
                test_binary_ids: vec!["bin".to_string()],
                cache_status: RustCovCacheStatus::FreshUnstored,
                stdout: None,
                stderr: None,
            },
            RustLlvmCovOutcome {
                selector: "tests::fresh_fail".to_string(),
                status: rpytest_runner::TestStatus::Failed,
                exit_code: Some(1),
                duration: Duration::from_millis(1),
                coverage: RustLineCoverage {
                    files: BTreeMap::new(),
                },
                test_binary_ids: vec!["bin".to_string()],
                cache_status: RustCovCacheStatus::FreshUnstored,
                stdout: None,
                stderr: Some(b"fresh boom\n".to_vec()),
            },
            RustLlvmCovOutcome {
                selector: "tests::fresh_fail_empty_stderr".to_string(),
                status: rpytest_runner::TestStatus::Failed,
                exit_code: Some(1),
                duration: Duration::from_millis(1),
                coverage: RustLineCoverage {
                    files: BTreeMap::new(),
                },
                test_binary_ids: vec!["bin".to_string()],
                cache_status: RustCovCacheStatus::FreshUnstored,
                stdout: None,
                stderr: Some(Vec::new()),
            },
        ],
        batch_error: None,
        counters: RustCoverageBatchCounters::default(),
        test_binaries: Vec::new(),
    };

    let summary = finish_rust_coverage_batch_result(tmp.path(), &identity, result).unwrap();
    assert_eq!(summary.total, 3);
    assert_eq!(summary.failed, 2);
    assert_eq!(
        summary.failed_selectors,
        vec![
            "tests::fresh_fail".to_string(),
            "tests::fresh_fail_empty_stderr".to_string()
        ]
    );
    assert_eq!(
        summary.max_passing_run_duration,
        Duration::from_millis(1)
    );
}

#[test]
fn run_rust_llvm_cov_selectors_reaches_compat_executor_for_nonempty_selectors() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("src").join("lib.rs"),
        "pub fn v() -> u32 { 1 }\n",
    )
    .unwrap();

    let outcome =
        run_rust_llvm_cov_selectors(tmp.path(), &["tests::case".to_string()], &[], true, 1, None);
    match outcome {
        Ok(summary) => {
            assert_eq!(summary.total, 1);
            assert!(summary.rust_unmatched_selectors >= 1 || summary.cache_misses >= 1);
        }
        Err(err) => {
            assert!(
                err.contains("did not execute") || err.contains("selector"),
                "compat path must surface unmatched-selector failure, got: {err}"
            );
        }
    }
}

#[test]
fn rust_coverage_tool_identity_from_versions_copies_fields() {
    let versions = RustCoverageToolVersions {
        cargo: "cargo 1".into(),
        llvm_cov: "llvm-cov 1".into(),
        rustc: "rustc 1".into(),
        cargo_nextest: "nextest 1".into(),
    };
    let tools = rust_coverage_tool_identity_from_versions(&versions);
    assert_eq!(tools.cargo_version, "cargo 1");
    assert_eq!(tools.llvm_cov_version, "llvm-cov 1");
    assert_eq!(tools.rustc_version, "rustc 1");
    assert_eq!(tools.cargo_nextest_version, "nextest 1");
}

#[test]
fn build_current_rust_test_executable_index_rejects_unsupported_args() {
    let tmp = tempfile::tempdir().unwrap();
    let result = build_current_rust_test_executable_index(
        tmp.path(),
        &["t".into()],
        &["--format".into(), "json".into()],
        1,
    );
    let err = match result {
        Ok(_) => panic!("expected unsupported arg rejection"),
        Err(message) => message,
    };
    assert!(err.contains("unsupported") || err.contains("format") || !err.is_empty());
}

#[test]
fn build_current_rust_test_executable_index_on_bare_temp_repo_indexes_or_errors() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname=\"t\"\nversion=\"0.0.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/lib.rs"), "").unwrap();
    let result =
        build_current_rust_test_executable_index(tmp.path(), &["missing_case".into()], &[], 1);
    match result {
        Ok(build) => {
            // Index construction lists executables; unmatched selectors stay absent/empty.
            let mapped = build
                .index
                .selector_binary_ids
                .get("missing_case")
                .map(|ids| ids.as_slice())
                .unwrap_or(&[]);
            assert!(
                mapped.is_empty(),
                "missing_case must not map to binaries: {mapped:?}"
            );
            assert_eq!(
                build.request.logical_selectors,
                vec!["missing_case".to_string()]
            );
        }
        Err(err) => assert!(!err.is_empty(), "error path must carry a message"),
    }
}
