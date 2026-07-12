use std::collections::{BTreeMap, BTreeSet};

use rpytest_runner::TestStatus;

use super::llvm_cov_json::{
    LlvmCovData, LlvmCovFile, LlvmCovReport, covered_line_from_segment, parse_llvm_cov_json,
    parse_llvm_cov_json_file,
};
use super::{
    RustCovCacheStatus, RustLineCoverage, RustLlvmCovError, RustLlvmCovOutcome,
};

#[test]
fn rust_llvm_cov_request_outcome_and_coverage_types_expose_expected_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let coverage = RustLineCoverage::witness();
    let outcome = RustLlvmCovOutcome::witness();

    assert_eq!(outcome.status, TestStatus::Passed);
    assert_eq!(outcome.cache_status, RustCovCacheStatus::witness_hit());
    assert_eq!(coverage.files["src/lib.rs"], BTreeSet::from([1, 2]));
    assert_eq!(outcome.coverage.files["src/lib.rs"], BTreeSet::from([1, 2]));
}

#[test]
fn llvm_cov_json_parsing_filters_workspace_paths_and_zero_counts() {
    let tmp = tempfile::tempdir().unwrap();
    let source_root = tmp.path();
    let covered = source_root.join("src").join("lib.rs");
    std::fs::create_dir_all(covered.parent().unwrap()).unwrap();
    std::fs::write(&covered, "fn main() {}\n").unwrap();
    let bytes = format!(
        r#"{{"data":[{{"files":[{{"filename":"{}","segments":[[1,1,1,true,true,false],[2,2,0,true,true,false]]}}]}}]}}"#,
        covered.display()
    );

    let parsed = parse_llvm_cov_json(bytes.as_bytes(), source_root).unwrap();

    assert_eq!(parsed.files.len(), 1);
    assert_eq!(
        parsed.files[&covered.to_string_lossy().to_string()],
        BTreeSet::from([1])
    );
}

#[test]
fn llvm_cov_json_file_parser_reports_missing_artifact() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("missing.json");

    let missing_err = parse_llvm_cov_json_file(&missing, tmp.path()).unwrap_err();

    assert!(matches!(missing_err, RustLlvmCovError::MissingArtifact(_)));
}

#[test]
fn llvm_cov_json_helpers_cover_report_shapes() {
    let report = LlvmCovReport::witness("src/lib.rs".to_string());
    let data = LlvmCovData::witness("src/lib.rs".to_string());
    let file = LlvmCovFile::witness("src/lib.rs".to_string());
    let line = covered_line_from_segment(vec![
        serde_json::Value::from(1),
        serde_json::Value::from(1),
        serde_json::Value::from(1),
    ]);

    assert_eq!(report.data.len(), 1);
    assert_eq!(data.files.len(), 1);
    assert_eq!(file.filename, "src/lib.rs");
    assert_eq!(line, Some(1));
}

#[test]
fn rust_cov_cache_entry_round_trips_outcome_fields() {
    let outcome = RustLlvmCovOutcome {
        selector: "alpha".to_string(),
        status: TestStatus::Passed,
        exit_code: Some(0),
        duration: std::time::Duration::from_millis(2),
        coverage: RustLineCoverage {
            files: BTreeMap::from([("src/lib.rs".to_string(), BTreeSet::from([1]))]),
        },
        cache_status: RustCovCacheStatus::MissStored,
        stdout: None,
        stderr: None,
    };
    let entry = super::rust_cov_cache::RustCovCacheEntry::from(&outcome);

    assert_eq!(entry.selector, "alpha");
    assert_eq!(entry.status, TestStatus::Passed);
    assert_eq!(entry.coverage.files["src/lib.rs"], BTreeSet::from([1]));
}
