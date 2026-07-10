use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use rpytest_runner::TestStatus;

use super::llvm_cov_json::{
    LlvmCovData, LlvmCovFile, LlvmCovReport, covered_line_from_segment, parse_llvm_cov_json,
    parse_llvm_cov_json_file,
};
use super::{
    CargoLlvmCovRunError, CargoLlvmCovRunOutcome, CargoLlvmCovRunRequest, CargoLlvmCovRunner,
    RustCovCacheStatus, RustLineCoverage, RustLlvmCov, RustLlvmCovError, RustLlvmCovOutcome,
    RustLlvmCovRequest, finalize, rust_cov_cache, worker,
};
use crate::test_support::write_demo_crate_source;

#[test]
fn rust_llvm_cov_request_outcome_and_coverage_types_expose_expected_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let req = RustLlvmCovRequest::witness(tmp.path());
    let coverage = RustLineCoverage::witness();
    let outcome = RustLlvmCovOutcome::witness();

    assert_eq!(req.selector, "smoke::passes");
    assert_eq!(req.cargo_args, vec!["--workspace"]);
    assert_eq!(req.test_args, vec!["--nocapture"]);
    assert_eq!(outcome.status, TestStatus::Passed);
    assert_eq!(outcome.cache_status, RustCovCacheStatus::witness_hit());
    assert_eq!(coverage.files["src/lib.rs"], BTreeSet::from([1, 2]));
    assert_eq!(outcome.coverage.files["src/lib.rs"], BTreeSet::from([1, 2]));
}

#[test]
fn rust_llvm_cov_validate_rust_cov_request_rejects_missing_selector_and_versions() {
    let tmp = tempfile::tempdir().unwrap();
    let valid = super::rust_cov_sample_request(tmp.path());
    assert!(super::validate_rust_cov_request(&valid).is_ok());

    let mut missing_selector = valid.clone();
    missing_selector.selector.clear();
    assert!(matches!(
        super::validate_rust_cov_request(&missing_selector),
        Err(RustLlvmCovError::InvalidRequest(message)) if message.contains("selector")
    ));

    let mut missing_llvm_cov = valid.clone();
    missing_llvm_cov.llvm_cov_version.clear();
    assert!(super::validate_rust_cov_request(&missing_llvm_cov).is_err());

    let mut missing_rustc = valid;
    missing_rustc.rustc_version.clear();
    assert!(super::validate_rust_cov_request(&missing_rustc).is_err());
}

#[test]
fn rust_llvm_cov_builds_cargo_llvm_cov_argv_with_selector_before_test_args() {
    let tmp = tempfile::tempdir().unwrap();
    let run_req = CargoLlvmCovRunRequest {
        selector: "my_test_filter".to_string(),
        cwd: tmp.path().to_path_buf(),
        cargo: PathBuf::from("cargo"),
        cargo_args: vec![
            "--workspace".to_string(),
            "-p".to_string(),
            "kiss-ai".to_string(),
        ],
        test_args: vec!["--exact".to_string(), "--nocapture".to_string()],
        env: BTreeMap::new(),
        artifact_path: tmp.path().join("coverage.json"),
    };

    let argv = super::build_llvm_cov_argv(&run_req);

    assert_eq!(
        argv,
        vec![
            "cargo",
            "llvm-cov",
            "test",
            "--json",
            "--output-path",
            &tmp.path().join("coverage.json").to_string_lossy(),
            "--no-clean",
            "--workspace",
            "-p",
            "kiss-ai",
            "my_test_filter",
            "--",
            "--exact",
            "--nocapture",
        ]
    );
}

#[test]
fn rust_llvm_cov_parser_uses_segments_and_reports_missing_artifacts() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src").join("lib.rs");
    fs::create_dir_all(src.parent().unwrap()).unwrap();
    fs::write(&src, "pub fn value() -> u32 { 1 }\n").unwrap();
    let json = format!(
        r#"{{"data":[{{"files":[{{"filename":"{}","segments":[[1,1,3,true,true,false],[2,1,0,true,true,false]]}}]}}]}}"#,
        src.display()
    );

    let coverage = parse_llvm_cov_json(json.as_bytes(), tmp.path()).unwrap();
    let missing =
        parse_llvm_cov_json_file(&tmp.path().join("missing.json"), tmp.path()).unwrap_err();

    assert_eq!(
        coverage.files[&src.canonicalize().unwrap().to_string_lossy().to_string()],
        BTreeSet::from([1])
    );
    assert!(matches!(missing, RustLlvmCovError::MissingArtifact(_)));
}

#[test]
fn rust_llvm_cov_segment_parser_requires_positive_line_and_count() {
    assert_eq!(
        covered_line_from_segment(vec![
            serde_json::Value::from(4),
            serde_json::Value::from(1),
            serde_json::Value::from(2),
        ]),
        Some(4)
    );
    assert_eq!(
        covered_line_from_segment(vec![
            serde_json::Value::from(4),
            serde_json::Value::from(1),
            serde_json::Value::from(0),
        ]),
        None
    );
    assert_eq!(
        covered_line_from_segment(vec![serde_json::Value::from(0)]),
        None
    );
}

#[test]
fn rust_llvm_cov_report_witness_preserves_file_segment_shape() {
    let report = LlvmCovReport::witness("src/lib.rs".to_string());
    let data = LlvmCovData::witness("src/data.rs".to_string());
    let file = LlvmCovFile::witness("src/file.rs".to_string());

    assert_eq!(report.data[0].files[0].filename, "src/lib.rs");
    assert_eq!(
        report.data[0].files[0].segments[0][0],
        serde_json::Value::from(1)
    );
    assert_eq!(data.files[0].filename, "src/data.rs");
    assert_eq!(file.filename, "src/file.rs");
    assert_eq!(file.segments[0][2], serde_json::Value::from(1));
}

#[test]
fn rust_llvm_cov_builds_isolated_cargo_runner_request() {
    let tmp = tempfile::tempdir().unwrap();
    let req = RustLlvmCovRequest {
        worker_slot: 3,
        ..super::rust_cov_sample_request(tmp.path())
    };
    let artifact = tmp.path().join("coverage.json");

    let run_req = super::build_cargo_runner_request(&req, &artifact);
    let worker = tmp
        .path()
        .join(".rust_llvm_cov_cache")
        .join("workers")
        .join("slot-3");

    assert_eq!(run_req.selector, req.selector);
    assert_eq!(run_req.artifact_path, artifact);
    assert_eq!(
        run_req.env["CARGO_TARGET_DIR"],
        worker.join("target").to_string_lossy()
    );
    assert_eq!(
        run_req.env["TMPDIR"],
        worker::rust_cov_worker_tmp_root(&req.cache_root, req.worker_slot).to_string_lossy()
    );
    assert!(!std::path::Path::new(&run_req.env["TMPDIR"]).starts_with(tmp.path()));
    assert!(run_req.env["LLVM_PROFILE_FILE"].contains("%m-%p.profraw"));
}

#[test]
fn rust_llvm_cov_cached_outcome_omits_output_but_keeps_status_and_coverage() {
    let entry = rust_cov_cache::RustCovCacheEntry::from(&RustLlvmCovOutcome {
        selector: "smoke::passes".to_string(),
        status: TestStatus::Passed,
        exit_code: Some(0),
        duration: Duration::from_millis(7),
        coverage: RustLineCoverage::witness(),
        cache_status: RustCovCacheStatus::MissStored,
        stdout: Some(b"fresh".to_vec()),
        stderr: Some(b"err".to_vec()),
    });

    let outcome = super::rust_cov_outcome_from_cache(entry);

    assert_eq!(outcome.selector, "smoke::passes");
    assert_eq!(outcome.status, TestStatus::Passed);
    assert_eq!(outcome.cache_status, RustCovCacheStatus::Hit);
    assert_eq!(outcome.stdout, None);
    assert_eq!(outcome.stderr, None);
    assert_eq!(outcome.coverage.files["src/lib.rs"], BTreeSet::from([1, 2]));
}

#[test]
fn rust_llvm_cov_run_or_reuse_uses_cache_and_force_rerun() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::create_dir(tmp.path().join("src")).unwrap();
    let lib = tmp.path().join("src").join("lib.rs");
    fs::write(&lib, "pub fn value() -> u32 { 1 }\n").unwrap();
    let calls = Rc::new(Cell::new(0));
    let runner = fake_runner(Rc::clone(&calls), lib);
    let cov = RustLlvmCov::new(runner);
    let req = super::rust_cov_sample_request(tmp.path());

    let first = cov.run_or_reuse(req.clone()).unwrap();
    let second = cov.run_or_reuse(req.clone()).unwrap();
    let forced = cov
        .run_or_reuse(RustLlvmCovRequest {
            force_rerun: true,
            ..req
        })
        .unwrap();

    assert_eq!(calls.get(), 2);
    assert_eq!(first.cache_status, RustCovCacheStatus::MissStored);
    assert_eq!(second.cache_status, RustCovCacheStatus::Hit);
    assert_eq!(second.stdout, None);
    assert_eq!(forced.cache_status, RustCovCacheStatus::MissStored);
}

#[test]
fn rust_llvm_cov_digest_and_artifact_helpers_are_collision_resistant() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join(".rust_llvm_cov_cache");
    fs::create_dir_all(&cache_root).unwrap();

    assert_eq!(worker::hex_lower(&[0x00, 0x7f, 0xff]), "007fff");
    assert_eq!(
        worker::os_str_bytes(std::ffi::OsStr::new("cache-root")),
        b"cache-root".to_vec()
    );
    assert_eq!(worker::cache_root_digest(&cache_root).len(), 64);
    let first = finalize::rust_cov_artifact_path(&cache_root, "abc");
    let second = finalize::rust_cov_artifact_path(&cache_root, "abc");

    assert_ne!(first, second);
    assert!(first.starts_with(cache_root.join("artifacts")));
    assert!(
        first
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("abc.")
    );
}

#[test]
fn rust_llvm_cov_lock_helpers_create_persistent_lock_files() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = tmp.path().join(".rust_llvm_cov_cache");

    drop(worker::lock_selector(&cache_root, "fingerprint").unwrap());
    drop(worker::lock_legacy_cleanup(&cache_root).unwrap());
    drop(worker::lock_worker(&cache_root, 3).unwrap());

    assert!(cache_root.join("locks/selectors/fingerprint.lock").exists());
    assert!(
        cache_root
            .join("locks/workers/legacy-cleanup.lock")
            .exists()
    );
    assert!(cache_root.join("locks/workers/slot-3.lock").exists());
}

#[test]
fn rust_llvm_cov_finalize_run_policies_store_and_cleanup_success_and_failure() {
    let tmp = tempfile::tempdir().unwrap();
    write_demo_crate_source(tmp.path());
    let mut req = super::rust_cov_sample_request(tmp.path());
    req.cache_root = tmp.path().join(".rust_llvm_cov_cache");
    fs::create_dir_all(&req.cache_root).unwrap();
    let artifact = finalize::rust_cov_artifact_path(&req.cache_root, "passed");
    fs::create_dir_all(artifact.parent().unwrap()).unwrap();
    fs::write(
        &artifact,
        format!(
            r#"{{"data":[{{"files":[{{"filename":"{}","segments":[[1,1,1,true,true,false]]}}]}}]}}"#,
            tmp.path().join("src").join("lib.rs").display()
        ),
    )
    .unwrap();
    fs::create_dir_all(worker::rust_cov_worker_tmp_root(
        &req.cache_root,
        req.worker_slot,
    ))
    .unwrap();

    let passed = finalize::finalize_run(
        &req,
        "passed",
        Ok(CargoLlvmCovRunOutcome {
            selector: req.selector.clone(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            stdout: Vec::new(),
            stderr: Vec::new(),
            artifact_path: artifact.clone(),
        }),
    )
    .unwrap();

    assert_eq!(passed.cache_status, RustCovCacheStatus::MissStored);
    assert!(!artifact.exists());
    assert!(!worker::rust_cov_worker_tmp_root(&req.cache_root, req.worker_slot).exists());

    let failed_artifact = finalize::rust_cov_artifact_path(&req.cache_root, "failed");
    let failed = finalize::finalize_run(
        &req,
        "failed",
        Ok(CargoLlvmCovRunOutcome {
            selector: req.selector.clone(),
            status: TestStatus::Failed,
            exit_code: Some(1),
            duration: Duration::from_millis(1),
            stdout: b"failed".to_vec(),
            stderr: Vec::new(),
            artifact_path: failed_artifact,
        }),
    )
    .unwrap();

    assert_eq!(failed.status, TestStatus::Failed);
    assert!(failed.coverage.files.is_empty());
}

#[test]
fn rust_llvm_cov_error_combiner_and_test_hook_preserve_failures() {
    let primary = RustLlvmCovError::InvalidRequest("primary".to_string());
    let combined = finalize::combine_primary_and_finalization(
        primary,
        vec![RustLlvmCovError::InvalidRequest("cleanup".to_string())],
    );

    assert!(matches!(
        combined,
        RustLlvmCovError::Composite { finalization, .. } if finalization.len() == 1
    ));
    assert!(worker::wait_at_unlocked_miss_hook().is_ok());
}

fn fake_runner(calls: Rc<Cell<usize>>, covered_file: PathBuf) -> CargoLlvmCovRunner {
    CargoLlvmCovRunner::from_fn(move |req| {
        calls.set(calls.get() + 1);
        if let Some(parent) = req.artifact_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let json = format!(
            r#"{{"data":[{{"files":[{{"filename":"{}","segments":[[1,1,{},true,true,false]]}}]}}]}}"#,
            covered_file.display(),
            calls.get()
        );
        fs::write(&req.artifact_path, json).unwrap();
        Ok(CargoLlvmCovRunOutcome {
            selector: req.selector,
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(2),
            stdout: format!("fresh stdout {}", calls.get()).into_bytes(),
            stderr: format!("fresh stderr {}", calls.get()).into_bytes(),
            artifact_path: req.artifact_path,
        })
    })
}

#[test]
fn rust_llvm_cov_error_type_wraps_runner_errors() {
    let err = RustLlvmCovError::from(CargoLlvmCovRunError::InvalidRequest(
        "bad selector".to_string(),
    ));

    assert!(matches!(err, RustLlvmCovError::Runner(_)));
}
