use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use crate::rpytest_runner::TestStatus;

use super::cache;
use super::{CacheStatus, LineCoverage, RslipOutcome, rslip_sample_request};

fn outcome() -> RslipOutcome {
    RslipOutcome {
        nodeid: "test_sample.py::test_ok".to_string(),
        status: TestStatus::Passed,
        exit_code: Some(0),
        duration: Duration::from_millis(3),
        coverage: LineCoverage {
            files: BTreeMap::from([("app.py".to_string(), BTreeSet::from([1, 2]))]),
        },
        cache_status: CacheStatus::MissStored,
        stdout: Some(b"out".to_vec()),
        stderr: Some(b"err".to_vec()),
    }
}

#[test]
fn rslip_cache_round_trips_entries_atomically() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("app.py"), "x = 1\n").unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let entry = cache::RslipCacheEntry::from_outcome(&outcome(), tmp.path());
    let req = rslip_sample_request(tmp.path());

    cache::store_rslip_cache_entry(&req, &entry).unwrap();
    let loaded = cache::load_rslip_cache_entry(&req).unwrap();

    assert_eq!(loaded.nodeid, "test_sample.py::test_ok");
    assert_eq!(loaded.status, TestStatus::Passed);
    assert_eq!(loaded.coverage.files["app.py"], BTreeSet::from([1, 2]));
    assert!(
        cache::python_records_dir(tmp.path()).ends_with(Path::new(".kiss/test/records/python"))
    );
    let mut missing = req.clone();
    missing.nodeid = "test_sample.py::missing".to_string();
    assert!(cache::load_rslip_cache_entry(&missing).is_none());
}

#[test]
fn record_written_under_another_toolchain_identity_is_not_loaded() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("app.py"), "x = 1\n").unwrap();
    let entry = cache::RslipCacheEntry::from_outcome(&outcome(), tmp.path());
    let req = rslip_sample_request(tmp.path());
    cache::store_rslip_cache_entry(&req, &entry).unwrap();

    let mut other = req.clone();
    other.python_version = "3.99.0".to_string();
    assert!(cache::load_rslip_cache_entry(&other).is_none());
    let mut other_host = req.clone();
    other_host.cache_root = tmp.path().join("other-host-cache");
    assert!(cache::load_rslip_cache_entry(&other_host).is_none());
    assert!(cache::load_rslip_cache_entry(&req).is_some());
}

#[test]
fn rslip_cache_rejects_duplicate_temp_file_creation() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("entry.tmp");

    let mut file = cache::create_new_rslip_cache_file(&path).unwrap();
    file.write_all(b"payload").unwrap();

    assert!(cache::create_new_rslip_cache_file(&path).is_err());
}

#[test]
fn rslip_cache_fingerprint_is_identity_only_not_source_tree() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("app.py"), "def value():\n    return 1\n").unwrap();
    let req = rslip_sample_request(tmp.path());
    let first = cache::rslip_cache_fingerprint(&req).unwrap();
    fs::write(tmp.path().join("app.py"), "def value():\n    return 2\n").unwrap();
    let source_changed = cache::rslip_cache_fingerprint(&req).unwrap();
    let mut version_changed = req;
    version_changed.python_version.push_str(" changed");
    let version_changed = cache::rslip_cache_fingerprint(&version_changed).unwrap();

    assert_eq!(first, source_changed);
    assert_ne!(source_changed, version_changed);
}

#[test]
fn rslip_cache_inputs_include_python_config_and_skip_cache_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(tmp.path().join(".rslip_cache")).unwrap();
    fs::create_dir(tmp.path().join(".kiss")).unwrap();
    fs::create_dir_all(tmp.path().join(".kiss").join("test").join("rslip_cache")).unwrap();
    fs::write(tmp.path().join("pytest.ini"), "[pytest]\n").unwrap();
    fs::write(tmp.path().join("app.py"), "x = 1\n").unwrap();
    fs::write(
        tmp.path().join(".rslip_cache").join("ignored.py"),
        "x = 2\n",
    )
    .unwrap();

    let names: BTreeSet<_> = cache::rslip_input_files(tmp.path())
        .unwrap()
        .into_iter()
        .map(|path| path.strip_prefix(tmp.path()).unwrap().to_path_buf())
        .collect();

    assert!(names.contains(Path::new("app.py")));
    assert!(names.contains(Path::new("pytest.ini")));
    assert!(!names.contains(Path::new(".rslip_cache/ignored.py")));
    assert!(cache::rslip_unique_suffix().contains('.'));
}

#[test]
fn rslip_cache_hash_helper_matches_fnv1a_examples() {
    assert_eq!(
        cache::rslip_fnv1a64(0xcbf2_9ce4_8422_2325, b"hello"),
        0xa430_d846_80aa_bd0b
    );
    assert_ne!(
        cache::rslip_fnv1a64(0xcbf2_9ce4_8422_2325, b"a"),
        cache::rslip_fnv1a64(0xcbf2_9ce4_8422_2325, b"b")
    );
}
