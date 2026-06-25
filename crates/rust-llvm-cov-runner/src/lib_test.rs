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
    RustLlvmCovRequest, build_cargo_runner_request, build_llvm_cov_argv, rust_cov_cache,
    rust_cov_outcome_from_cache, rust_cov_sample_request, validate_rust_cov_request,
};

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
    let valid = rust_cov_sample_request(tmp.path());
    assert!(validate_rust_cov_request(&valid).is_ok());

    let mut missing_selector = valid.clone();
    missing_selector.selector.clear();
    assert!(matches!(
        validate_rust_cov_request(&missing_selector),
        Err(RustLlvmCovError::InvalidRequest(message)) if message.contains("selector")
    ));

    let mut missing_llvm_cov = valid.clone();
    missing_llvm_cov.llvm_cov_version.clear();
    assert!(validate_rust_cov_request(&missing_llvm_cov).is_err());

    let mut missing_rustc = valid;
    missing_rustc.rustc_version.clear();
    assert!(validate_rust_cov_request(&missing_rustc).is_err());
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

    let argv = build_llvm_cov_argv(&run_req);

    assert_eq!(
        argv,
        vec![
            "cargo",
            "llvm-cov",
            "test",
            "--json",
            "--output-path",
            &tmp.path().join("coverage.json").to_string_lossy(),
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
    let req = rust_cov_sample_request(tmp.path());
    let artifact = tmp.path().join("coverage.json");
    let worker = tmp.path().join("worker-1");

    let run_req = build_cargo_runner_request(&req, &artifact, &worker);

    assert_eq!(run_req.selector, req.selector);
    assert_eq!(run_req.artifact_path, artifact);
    assert_eq!(
        run_req.env["CARGO_TARGET_DIR"],
        worker.join("target").to_string_lossy()
    );
    assert_eq!(run_req.env["TMPDIR"], worker.join("tmp").to_string_lossy());
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

    let outcome = rust_cov_outcome_from_cache(entry);

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
    let req = rust_cov_sample_request(tmp.path());

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
