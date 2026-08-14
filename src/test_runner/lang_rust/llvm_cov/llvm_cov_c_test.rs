use super::*;
use super::tests::{passed_rust_llvm_cov_outcome, write_rust_test_crate};
use rust_llvm_cov_runner::RustCoverageBatchCounters;

fn default_tool_versions() -> RustCoverageToolVersions {
    RustCoverageToolVersions {
        cargo: "cargo 1.88.0".to_string(),
        llvm_cov: "cargo-llvm-cov 0.6.0".to_string(),
        rustc: "rustc 1.88.0".to_string(),
        cargo_nextest: "cargo-nextest 0.9.0".to_string(),
    }
}

fn default_run_options<'a>() -> RustCoverageRunOptions<'a> {
    RustCoverageRunOptions {
        extra: &[],
        force_rerun: true,
        jobs: 1,
        population_publication_selectors: None,
        coverage_output_mode: rust_llvm_cov_runner::CoverageOutputMode::SelectorEntries,
        gate: kiss::GateConfig::default(),
    }
}

#[test]
fn rust_llvm_cov_stage_emits_before_pass() {
    let tmp = tempfile::tempdir().unwrap();
    write_rust_test_crate(tmp.path(), &["case"]);
    let selectors = vec!["tests::case".to_string()];
    let out = crate::test_runner::capture_stdout::capture_stdout(|| {
        let summary = run_rust_llvm_cov_selectors_with_deps(
            tmp.path(),
            &selectors,
            default_run_options(),
            |_repo_root| Ok(default_tool_versions()),
            |batch_req, _versions| {
                Ok(RustCoverageBatchResult {
                    completed: batch_req
                        .logical_selectors
                        .iter()
                        .cloned()
                        .map(passed_rust_llvm_cov_outcome)
                        .collect(),
                    batch_error: None,
                    counters: RustCoverageBatchCounters::default(),
                    test_binaries: Vec::new(),
                })
            },
        )
        .unwrap();
        assert_eq!(summary.total, 1);
    });
    let stage_idx = out
        .find("kiss test: stage rust_llvm_cov")
        .expect("rust_llvm_cov stage line");
    let pass_idx = out.find("PASS:").expect("PASS line from finish");
    assert!(
        stage_idx < pass_idx,
        "rust_llvm_cov must precede PASS on stdout:\n{out}"
    );
}

#[test]
fn empty_selectors_do_not_emit_rust_llvm_cov_stage() {
    let tmp = tempfile::tempdir().unwrap();
    let out = crate::test_runner::capture_stdout::capture_stdout(|| {
        let summary = run_rust_llvm_cov_selectors_with_deps(
            tmp.path(),
            &[],
            default_run_options(),
            |_repo_root| unreachable!("empty selectors must not detect tools"),
            |_batch_req, _versions| unreachable!("empty selectors must not execute"),
        )
        .unwrap();
        assert_eq!(summary.total, 0);
    });
    assert!(
        !out.contains("rust_llvm_cov"),
        "empty selectors must not emit rust_llvm_cov:\n{out}"
    );
}

#[test]
fn executor_error_does_not_emit_rust_llvm_cov_stage() {
    let tmp = tempfile::tempdir().unwrap();
    write_rust_test_crate(tmp.path(), &["case"]);
    let selectors = vec!["tests::case".to_string()];
    let out = crate::test_runner::capture_stdout::capture_stdout(|| {
        let err = run_rust_llvm_cov_selectors_with_deps(
            tmp.path(),
            &selectors,
            default_run_options(),
            |_repo_root| Ok(default_tool_versions()),
            |_batch_req, _versions| Err("executor failed".to_string()),
        )
        .unwrap_err();
        assert!(err.contains("executor failed"));
    });
    assert!(
        !out.contains("rust_llvm_cov"),
        "executor Err must not emit rust_llvm_cov:\n{out}"
    );
    assert!(
        !out.contains("PASS:"),
        "executor Err must not print PASS:\n{out}"
    );
}

#[test]
fn rust_coverage_batch_request_fails_closed_without_report_ids() {
    let tmp = tempfile::tempdir().unwrap();
    // Empty package: no #[test] symbols → map omits the requested selector.
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname=\"demo\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("src/lib.rs"), "pub fn x() {}\n").unwrap();
    let err = rust_coverage_batch_request_from_parts(
        tmp.path(),
        &["tests::case".to_string()],
        &[],
        false,
        1,
        None,
        rust_llvm_cov_runner::CoverageOutputMode::SelectorEntries,
        &kiss::GateConfig::default(),
    )
    .unwrap_err();
    assert!(
        err.contains("missing PATH::symbol report id"),
        "unexpected err: {err}"
    );
}

#[test]
fn rust_coverage_batch_request_applies_path_limits_with_report_ids() {
    let tmp = tempfile::tempdir().unwrap();
    write_rust_test_crate(tmp.path(), &["case"]);
    let gate = kiss::GateConfig {
        max_unit_test_seconds: vec![
            ("src/lib.rs".to_string(), 10.0),
            ("*".to_string(), 0.0),
        ],
        ..Default::default()
    };
    let req = rust_coverage_batch_request_from_parts(
        tmp.path(),
        &["tests::case".to_string()],
        &[],
        false,
        1,
        None,
        rust_llvm_cov_runner::CoverageOutputMode::SelectorEntries,
        &gate,
    )
    .unwrap();
    assert_eq!(
        req.selector_timeout_millis.get("tests::case"),
        Some(&10_000)
    );
}
