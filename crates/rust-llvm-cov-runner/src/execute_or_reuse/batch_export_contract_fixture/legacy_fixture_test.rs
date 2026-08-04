use super::*;
use crate::{RustCovCacheStatus, RustLineCoverage, RustLlvmCovOutcome};
use rpytest_runner::TestStatus;
use std::time::Duration;

#[test]
fn batch_request_helpers_are_constructible() {
    let batch = batch_request(
        Path::new("/tmp"),
        &["alpha".to_string()],
        Path::new("/tmp/helper"),
    );
    assert_eq!(batch.logical_selectors, vec!["alpha".to_string()]);
    let batch_with_args = batch_request_with_args(
        Path::new("/tmp"),
        &["alpha".to_string()],
        Path::new("/tmp/helper"),
        &["--ignored".to_string()],
        4,
    );
    assert_eq!(batch_with_args.test_args, ["--ignored"]);
    assert_eq!(batch_with_args.jobs, 4);
}

#[test]
fn per_selector_oracle_uses_fake_cargo_for_fast_coverage() {
    let tmp = tempfile::tempdir().unwrap();
    let fake_cargo = tmp.path().join("fake-cargo");
    let lib = Path::new(FIXTURE_ROOT).join("runner/src/lib.rs");
    std::fs::write(
        &fake_cargo,
        format!(
            "#!/bin/sh\n\
             if [ \"$1\" = llvm-cov ] && [ \"$2\" = test ]; then\n\
               out=\"\"\n\
               while [ $# -gt 0 ]; do\n\
                 if [ \"$1\" = --output-path ]; then out=\"$2\"; shift; fi\n\
                 shift\n\
               done\n\
               printf '%s' '{{\"data\":[{{\"files\":[{{\"filename\":\"{}\",\"segments\":[[1,1,1,true,true,false]]}}]}}]}}' >\"$out\"\n\
               exit 0\n\
             fi\n\
             exit 1\n",
            lib.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&fake_cargo).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_cargo, permissions).unwrap();
    }
    // SAFETY: test-local env override for oracle cargo program.
    unsafe {
        std::env::set_var("KISS_ORACLE_CARGO", &fake_cargo);
    }
    let tools = RustCoverageToolIdentity {
        cargo_version: "cargo".to_string(),
        llvm_cov_version: "llvm-cov".to_string(),
        rustc_version: "rustc".to_string(),
        cargo_nextest_version: "nextest".to_string(),
    };
    let outcome = run_legacy_selector_with_args(
        "alpha",
        &tools,
        Path::new("/tmp/helper"),
        tmp.path().join("cache"),
        &[],
    );
    // SAFETY: restore default oracle cargo program selection.
    unsafe {
        std::env::remove_var("KISS_ORACLE_CARGO");
    }
    assert_eq!(outcome.selector, "alpha");
    assert_eq!(outcome.status, TestStatus::Passed);
    assert_eq!(outcome.exit_code, Some(0));
    assert!(!outcome.coverage.files.is_empty());
}

#[test]
fn parse_oracle_stdout_coverage_handles_empty_and_json_payload() {
    let fixture_root = Path::new(FIXTURE_ROOT);
    let source_root = fixture_root.join("runner");
    let empty = parse_oracle_stdout_coverage_for_test(&[], &source_root);
    assert!(empty.files.is_empty());
    let payload = br#"noise{"data":[{"files":[{"filename":"x","segments":[]}]}]}"#;
    let parsed = parse_oracle_stdout_coverage_for_test(payload, &source_root);
    assert!(parsed.files.is_empty());
}

#[test]
fn worker_tmp_parent_and_digest_helpers_are_stable() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join("cache");
    std::fs::create_dir_all(&cache_root).unwrap();
    let parent = crate::execute_or_reuse::worker::rust_cov_cache_tmp_parent(&cache_root);
    assert!(parent.to_string_lossy().contains("kiss-rust-llvm-cov"));
    assert_eq!(crate::execute_or_reuse::worker::hex_lower(&[0xab, 0xcd]), "abcd");
    assert_eq!(
        crate::execute_or_reuse::worker::os_str_bytes(std::ffi::OsStr::new("ab")),
        b"ab".to_vec()
    );
}

#[test]
fn failure_outcome_matching_allows_legacy_non_one_exit_codes() {
    let legacy = RustLlvmCovOutcome {
        selector: "fail".to_string(),
        status: TestStatus::Failed,
        exit_code: Some(101),
        duration: Duration::ZERO,
        coverage: RustLineCoverage {
            files: BTreeMap::new(),
        },
        test_binary_ids: vec!["test-bin".to_string()],
        cache_status: RustCovCacheStatus::MissStored,
        stdout: None,
        stderr: None,
    };
    let batch = RustLlvmCovOutcome {
        selector: "fail".to_string(),
        status: TestStatus::Failed,
        exit_code: Some(1),
        duration: Duration::ZERO,
        coverage: RustLineCoverage {
            files: BTreeMap::new(),
        },
        test_binary_ids: vec!["test-bin".to_string()],
        cache_status: RustCovCacheStatus::MissStored,
        stdout: None,
        stderr: None,
    };
    assert_outcomes_match(
        "fail",
        &legacy,
        &batch,
        Path::new(FIXTURE_ROOT),
        "unit-test",
    );
}
