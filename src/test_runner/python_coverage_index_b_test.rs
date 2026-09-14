use super::*;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use kiss::rpytest_runner::TestStatus;
use kiss::rslip::LineCoverage;

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
        "schema_version": kiss::rslip::CACHE_SCHEMA_VERSION,
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
    // Assert serialization only: blocked while held, then acquires after drop.
    // Do not bound post-release latency — under parallel `kiss test` load that
    // bound flaked (elapsed included the probe window + scheduler delay).
    let tmp = tempfile::tempdir().unwrap();
    let cache_root = python_coverage_cache_root(tmp.path()).unwrap();
    fs::create_dir_all(&cache_root).unwrap();
    let guard = kiss::rslip::lock_rslip_derived_state(&cache_root).unwrap();
    let cache_root_for_thread = cache_root.clone();
    let (tx, rx) = mpsc::channel();
    let waiter = thread::spawn(move || {
        let _second = kiss::rslip::lock_rslip_derived_state(&cache_root_for_thread).unwrap();
        let _ = tx.send(());
    });

    assert!(
        rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "second lock must not complete while the first guard is held"
    );
    drop(guard);
    rx.recv_timeout(Duration::from_secs(2))
        .expect("waiter should acquire the derived lock after the first guard drops");
    waiter.join().unwrap();
}
