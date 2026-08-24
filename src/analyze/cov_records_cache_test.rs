use super::{CovRecordsCacheKey, store_cov_records, try_load_cov_records};
use crate::analyze::line_coverage::LineCoverageRecord;
use crate::test_runner::check_line_coverage::RequiredCoverageLanguages;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

fn write_python_population(repo: &std::path::Path) {
    crate::analyze::cov_cache_test_support::write_python_population_for_cache_tests(repo, "abc");
}

fn write_rust_aggregate(repo: &std::path::Path) {
    let cache = repo.join(".kiss/rust_llvm_cov_cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("check_aggregate.json"),
        r#"{
            "integrity_fingerprint":"int1",
            "input_fingerprint":"in1",
            "generation_fingerprint":"gen1"
        }"#,
    )
    .unwrap();
}

fn touch_source(path: &std::path::Path, body: &str) -> PathBuf {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
    path.to_path_buf()
}

#[test]
fn cov_records_cache_round_trip_hits_then_misses_on_source_change() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    write_python_population(repo);
    write_rust_aggregate(repo);
    let py = touch_source(&repo.join("pkg/a.py"), "x = 1\n");
    let rs = touch_source(&repo.join("src/lib.rs"), "fn f() {}\n");
    let records = vec![LineCoverageRecord {
        file: py.clone(),
        total_lines: 1,
        covered_lines: 1,
        percent: 100,
        first_uncovered_line: None,
    }];
    let key = CovRecordsCacheKey {
        repo_root: repo,
        py_files: std::slice::from_ref(&py),
        rs_files: std::slice::from_ref(&rs),
        required: RequiredCoverageLanguages {
            python: true,
            rust: true,
        },
        threshold: 90,
        bypass_gate: false,
        ignore: &[],
        lang_filter: None,
    };
    assert!(try_load_cov_records(&key).is_none());
    store_cov_records(&key, &records);
    let loaded = try_load_cov_records(&key).expect("warm hit");
    assert_eq!(loaded, records);

    std::thread::sleep(Duration::from_millis(5));
    fs::write(&py, "x = 2\n").unwrap();
    let _ = fs::File::options()
        .write(true)
        .open(&py)
        .unwrap()
        .set_modified(SystemTime::now())
        .ok();
    assert!(try_load_cov_records(&key).is_none());
}

#[test]
fn cov_records_cache_misses_when_rust_backend_identity_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    write_python_population(repo);
    write_rust_aggregate(repo);
    let py = touch_source(&repo.join("a.py"), "pass\n");
    let rs = touch_source(&repo.join("lib.rs"), "fn g() {}\n");
    let key = CovRecordsCacheKey {
        repo_root: repo,
        py_files: std::slice::from_ref(&py),
        rs_files: std::slice::from_ref(&rs),
        required: RequiredCoverageLanguages {
            python: true,
            rust: true,
        },
        threshold: 80,
        bypass_gate: true,
        ignore: &[],
        lang_filter: Some("rust"),
    };
    store_cov_records(
        &key,
        &[LineCoverageRecord {
            file: rs.clone(),
            total_lines: 1,
            covered_lines: 0,
            percent: 0,
            first_uncovered_line: Some(1),
        }],
    );
    assert!(try_load_cov_records(&key).is_some());
    fs::write(
        repo.join(".kiss/rust_llvm_cov_cache/check_aggregate.json"),
        r#"{
            "integrity_fingerprint":"int2",
            "input_fingerprint":"in1",
            "generation_fingerprint":"gen1"
        }"#,
    )
    .unwrap();
    assert!(try_load_cov_records(&key).is_none());
}

#[test]
fn cov_records_cache_hits_with_rust_population_when_aggregate_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    write_python_population(repo);
    let cache = repo.join(".kiss/rust_llvm_cov_cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(
        cache.join("population.json"),
        r#"{
            "schema_version":"rust-llvm-cov-population-v3",
            "input_fingerprint":"in-pop",
            "generation_fingerprint":"gen-pop",
            "entries_fingerprint":"ent-pop",
            "selectors":["tests::one","tests::two"]
        }"#,
    )
    .unwrap();
    let py = touch_source(&repo.join("a.py"), "pass\n");
    let rs = touch_source(&repo.join("lib.rs"), "fn g() {}\n");
    let records = vec![LineCoverageRecord {
        file: rs.clone(),
        total_lines: 1,
        covered_lines: 1,
        percent: 100,
        first_uncovered_line: None,
    }];
    let key = CovRecordsCacheKey {
        repo_root: repo,
        py_files: std::slice::from_ref(&py),
        rs_files: std::slice::from_ref(&rs),
        required: RequiredCoverageLanguages {
            python: true,
            rust: true,
        },
        threshold: 90,
        bypass_gate: false,
        ignore: &[],
        lang_filter: None,
    };
    store_cov_records(&key, &records);
    let loaded = try_load_cov_records(&key).expect("population-backed warm hit");
    assert_eq!(loaded, records);
}
