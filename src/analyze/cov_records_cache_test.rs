use super::{
    CovRecordsCacheKey, lock_cache_for, mark_cached_records_orphan_clean, store_cov_records,
    try_load_cov_records, try_load_cov_records_with_orphan_state,
};
use crate::analyze::line_coverage::LineCoverageRecord;
use crate::test_runner::check_line_coverage::RequiredCoverageLanguages;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

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
        pytest_args: &[],
    };
    assert!(try_load_cov_records(&key).is_none());
    store_cov_records(&key, &records);
    let loaded = try_load_cov_records(&key).expect("warm hit");
    assert_eq!(loaded, records);
    let different_pytest_args = vec!["--tb=short".to_string()];
    let different_context = CovRecordsCacheKey {
        pytest_args: &different_pytest_args,
        ..key.clone()
    };
    assert!(
        try_load_cov_records(&different_context).is_none(),
        "coverage records must be bound to the current pytest execution context"
    );
    assert_eq!(try_load_cov_records_with_orphan_state(&key).unwrap().1, "");
    mark_cached_records_orphan_clean(&key, "policy");
    assert_eq!(
        try_load_cov_records_with_orphan_state(&key).unwrap().1,
        "policy"
    );
    store_cov_records(&key, &records);
    assert_eq!(
        try_load_cov_records_with_orphan_state(&key).unwrap().1,
        "policy",
        "same-fingerprint stores must preserve validated orphan policy"
    );

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
    fs::write(
        repo.join(".kiss/rust_llvm_cov_cache/execution_witness.json"),
        "{}",
    )
    .unwrap();
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
        pytest_args: &[],
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
fn cov_records_cache_hashes_witness_contents_not_only_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    write_rust_aggregate(repo);
    let witness = repo.join(".kiss/rust_llvm_cov_cache/execution_witness.json");
    fs::write(&witness, "{}").unwrap();
    let original_mtime = fs::metadata(&witness).unwrap().modified().unwrap();
    let rs = touch_source(&repo.join("lib.rs"), "fn g() {}\n");
    let key = CovRecordsCacheKey {
        repo_root: repo,
        py_files: &[],
        rs_files: std::slice::from_ref(&rs),
        required: RequiredCoverageLanguages {
            python: false,
            rust: true,
        },
        threshold: 80,
        bypass_gate: true,
        ignore: &[],
        lang_filter: Some("rust"),
        pytest_args: &[],
    };
    store_cov_records(&key, &[]);
    assert!(try_load_cov_records(&key).is_some());

    fs::write(&witness, "[]").unwrap();
    fs::File::options()
        .write(true)
        .open(&witness)
        .unwrap()
        .set_modified(original_mtime)
        .unwrap();
    assert!(
        try_load_cov_records(&key).is_none(),
        "same-length, same-mtime witness replacement must invalidate records"
    );
}

#[test]
fn cov_records_cache_hashes_source_contents_not_only_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    write_rust_aggregate(repo);
    let rs = touch_source(&repo.join("lib.rs"), "fn a() {}\n");
    let original_mtime = fs::metadata(&rs).unwrap().modified().unwrap();
    let key = CovRecordsCacheKey {
        repo_root: repo,
        py_files: &[],
        rs_files: std::slice::from_ref(&rs),
        required: RequiredCoverageLanguages {
            python: false,
            rust: true,
        },
        threshold: 80,
        bypass_gate: true,
        ignore: &[],
        lang_filter: Some("rust"),
        pytest_args: &[],
    };
    store_cov_records(&key, &[]);
    assert!(try_load_cov_records(&key).is_some());

    fs::write(&rs, "fn b() {}\n").unwrap();
    fs::File::options()
        .write(true)
        .open(&rs)
        .unwrap()
        .set_modified(original_mtime)
        .unwrap();
    assert!(
        try_load_cov_records(&key).is_none(),
        "same-length, same-mtime source replacement must invalidate records"
    );
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
        pytest_args: &[],
    };
    store_cov_records(&key, &records);
    let loaded = try_load_cov_records(&key).expect("population-backed warm hit");
    assert_eq!(loaded, records);
}

#[test]
fn cov_records_cache_misses_when_rust_generation_or_witness_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    write_python_population(repo);
    let cache = repo.join(".kiss/rust_llvm_cov_cache");
    fs::create_dir_all(&cache).unwrap();
    let witness = cache.join("execution_witness.json");
    fs::write(&witness, "{\"v\":1}\n").unwrap();
    let generation = cache.join("current_generation.json");
    fs::write(&generation, "{\"generation\":\"one\"}\n").unwrap();
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
        bypass_gate: false,
        ignore: &[],
        lang_filter: None,
        pytest_args: &[],
    };
    store_cov_records(
        &key,
        &[LineCoverageRecord {
            file: rs.clone(),
            total_lines: 1,
            covered_lines: 1,
            percent: 100,
            first_uncovered_line: None,
        }],
    );
    assert!(try_load_cov_records(&key).is_some());
    fs::write(&generation, "{\"generation\":\"two\"}\n").unwrap();
    assert!(try_load_cov_records(&key).is_none());
    store_cov_records(
        &key,
        &[LineCoverageRecord {
            file: rs.clone(),
            total_lines: 1,
            covered_lines: 1,
            percent: 100,
            first_uncovered_line: None,
        }],
    );
    assert!(try_load_cov_records(&key).is_some());
    std::thread::sleep(Duration::from_millis(5));
    fs::write(&witness, "{\"v\":2}\n").unwrap();
    let _ = fs::File::options()
        .write(true)
        .open(&witness)
        .unwrap()
        .set_modified(SystemTime::now())
        .ok();
    assert!(try_load_cov_records(&key).is_none());
}

#[test]
fn cov_records_cache_misses_when_cargo_toml_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    write_python_population(repo);
    write_rust_aggregate(repo);
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
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
        bypass_gate: false,
        ignore: &[],
        lang_filter: None,
        pytest_args: &[],
    };
    store_cov_records(
        &key,
        &[LineCoverageRecord {
            file: rs.clone(),
            total_lines: 1,
            covered_lines: 1,
            percent: 100,
            first_uncovered_line: None,
        }],
    );
    assert!(try_load_cov_records(&key).is_some());
    fs::write(
        repo.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[[bin]]\nname = \"tool\"\npath = \"lib.rs\"\n",
    )
    .unwrap();
    assert!(try_load_cov_records(&key).is_none());
}

#[test]
fn concurrent_record_writers_publish_one_complete_cache() {
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
        bypass_gate: false,
        ignore: &[],
        lang_filter: None,
        pytest_args: &[],
    };
    let records = vec![LineCoverageRecord {
        file: rs.clone(),
        total_lines: 1,
        covered_lines: 1,
        percent: 100,
        first_uncovered_line: None,
    }];
    let barrier = std::sync::Barrier::new(3);
    std::thread::scope(|scope| {
        for _ in 0..2 {
            scope.spawn(|| {
                barrier.wait();
                store_cov_records(&key, &records);
            });
        }
        barrier.wait();
    });
    assert_eq!(try_load_cov_records(&key), Some(records));
    let cache_dir = repo.join(".kiss");
    assert!(fs::read_dir(cache_dir).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")
    }));
}

#[test]
fn record_cache_lock_wait_is_bounded() {
    let tmp = tempfile::tempdir().unwrap();
    let _held = lock_cache_for(tmp.path(), Duration::from_millis(50)).unwrap();
    let started = Instant::now();
    assert!(lock_cache_for(tmp.path(), Duration::from_millis(75)).is_none());
    assert!(started.elapsed() < Duration::from_secs(1));
}
