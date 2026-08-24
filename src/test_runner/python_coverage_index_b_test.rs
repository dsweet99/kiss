use super::*;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use rpytest_runner::TestStatus;
use rslip::LineCoverage;

#[test]
fn legacy_unscoped_python_cache_entries_are_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let app = tmp.path().join("app.py");
    fs::write(&app, "def value():\n    return 1\n").unwrap();
    let legacy_entry = tmp
        .path()
        .join(".kiss")
        .join("rslip_cache")
        .join("entries")
        .join("legacy.json");
    fs::create_dir_all(legacy_entry.parent().unwrap()).unwrap();
    let entry = serde_json::json!({
        "schema_version": rslip::CACHE_SCHEMA_VERSION,
        "nodeid": "tests/test_app.py::test_value",
        "status": TestStatus::Passed,
        "exit_code": 0,
        "duration": Duration::from_millis(1),
        "coverage": LineCoverage {
            files: BTreeMap::from([(app.to_string_lossy().to_string(), BTreeSet::from([1]))]),
        },
    });
    fs::write(legacy_entry, serde_json::to_vec(&entry).unwrap()).unwrap();

    let index = rebuild_python_coverage_index(tmp.path()).unwrap();

    assert!(index.is_empty());
}

#[test]
fn derived_state_publication_waits_for_derived_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = python_coverage_cache_root(tmp.path()).unwrap();
    fs::create_dir_all(&cache_root).unwrap();
    let (lock_held_tx, lock_held_rx) = mpsc::channel();
    let guard = rslip::lock_rslip_derived_state(&cache_root).unwrap();
    lock_held_tx.send(()).unwrap();
    let cache_root_for_thread = cache_root.clone();
    let (tx, rx) = mpsc::channel();
    let waiter = thread::spawn(move || {
        lock_held_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let started = Instant::now();
        let _second = rslip::lock_rslip_derived_state(&cache_root_for_thread).unwrap();
        tx.send(started.elapsed()).unwrap();
    });

    assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
    drop(guard);
    let wait = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(wait < Duration::from_millis(500));
    waiter.join().unwrap();
}
