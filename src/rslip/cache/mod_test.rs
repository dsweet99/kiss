use super::*;
use std::collections::BTreeSet;

#[test]
fn rslip_cache_fingerprint_stable_when_unrelated_python_changes() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("pkg.py"), "def value():\n    return 1\n").unwrap();
    let req = crate::rslip::rslip_sample_request(tmp.path());
    let first = rslip_cache_fingerprint(&req).unwrap();
    fs::write(tmp.path().join("pkg.py"), "def value():\n    return 2\n").unwrap();
    let second = rslip_cache_fingerprint(&req).unwrap();
    assert_eq!(first, second);
}

#[test]
fn rslip_cache_fingerprint_changes_when_python_version_changes() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("pkg.py"), "def value():\n    return 1\n").unwrap();
    let mut req = crate::rslip::rslip_sample_request(tmp.path());
    let first = rslip_cache_fingerprint(&req).unwrap();
    req.python_version = "3.13.0".to_string();
    let second = rslip_cache_fingerprint(&req).unwrap();

    assert_ne!(first, second);
}

#[test]
fn coverage_gated_reuse_hits_when_covered_files_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    let app = tmp.path().join("app.py");
    let test = tmp.path().join("test_sample.py");
    fs::write(&app, "x = 1\n").unwrap();
    fs::write(&test, "def test_ok():\n    assert True\n").unwrap();
    let coverage = LineCoverage {
        files: BTreeMap::from([(app.to_string_lossy().into_owned(), BTreeSet::from([1]))]),
    };
    let entry = RslipCacheEntry {
        schema_version: CACHE_SCHEMA_VERSION.to_string(),
        nodeid: "test_sample.py::test_ok".to_string(),
        status: TestStatus::Passed,
        exit_code: Some(0),
        duration: std::time::Duration::from_millis(1),
        coverage: coverage.clone(),
        covered_digests: covered_file_digests(tmp.path(), "test_sample.py::test_ok", &coverage)
            .unwrap(),
    };
    assert!(entry_is_reusable(&entry, tmp.path()));
    fs::write(tmp.path().join("other.py"), "y = 2\n").unwrap();
    assert!(entry_is_reusable(&entry, tmp.path()));
    fs::write(&app, "x = 2\n").unwrap();
    assert!(!entry_is_reusable(&entry, tmp.path()));
}

#[test]
fn empty_coverage_is_never_reusable() {
    let tmp = tempfile::tempdir().unwrap();
    let entry = RslipCacheEntry {
        schema_version: CACHE_SCHEMA_VERSION.to_string(),
        nodeid: "test_sample.py::test_ok".to_string(),
        status: TestStatus::Passed,
        exit_code: Some(0),
        duration: std::time::Duration::from_millis(1),
        coverage: LineCoverage {
            files: BTreeMap::new(),
        },
        covered_digests: BTreeMap::new(),
    };
    assert!(!entry_is_reusable(&entry, tmp.path()));
}

#[test]
fn failed_status_is_never_reusable_even_with_coverage() {
    let tmp = tempfile::tempdir().unwrap();
    let app = tmp.path().join("app.py");
    fs::write(&app, "x = 1\n").unwrap();
    let coverage = LineCoverage {
        files: BTreeMap::from([(app.to_string_lossy().into_owned(), BTreeSet::from([1]))]),
    };
    let entry = RslipCacheEntry::from_outcome(
        &crate::rslip::RslipOutcome {
            nodeid: "t::f".into(),
            status: TestStatus::Failed,
            exit_code: Some(1),
            duration: std::time::Duration::from_millis(1),
            coverage,
            cache_status: crate::rslip::CacheStatus::MissStored,
            stdout: None,
            stderr: None,
        },
        tmp.path(),
    );
    assert!(!entry.coverage.files.is_empty());
    assert!(!entry_is_reusable(&entry, tmp.path()));
}

#[test]
fn conservative_inputs_include_pytest_config_and_skip_cache_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(tmp.path().join(".rslip_cache")).unwrap();
    fs::create_dir(tmp.path().join(".kiss")).unwrap();
    fs::create_dir(tmp.path().join(".kiss").join("rslip_cache")).unwrap();
    fs::write(tmp.path().join("pytest.ini"), "[pytest]\n").unwrap();
    fs::write(tmp.path().join("a.py"), "x = 1\n").unwrap();
    fs::write(
        tmp.path().join(".rslip_cache").join("ignored.py"),
        "x = 2\n",
    )
    .unwrap();
    fs::write(
        tmp.path()
            .join(".kiss")
            .join("rslip_cache")
            .join("ignored.py"),
        "x = 3\n",
    )
    .unwrap();

    let names: BTreeSet<_> = rslip_input_files(tmp.path())
        .unwrap()
        .into_iter()
        .map(|path| path.strip_prefix(tmp.path()).unwrap().to_path_buf())
        .collect();

    assert!(names.contains(Path::new("a.py")));
    assert!(names.contains(Path::new("pytest.ini")));
    assert!(!names.contains(Path::new(".rslip_cache/ignored.py")));
    assert!(!names.contains(Path::new(".kiss/rslip_cache/ignored.py")));
}

#[test]
fn helper_hash_and_temp_suffix_are_usable() {
    assert_ne!(rslip_unique_suffix(), "");
    assert_eq!(
        rslip_fnv1a64(0xcbf2_9ce4_8422_2325, b""),
        0xcbf2_9ce4_8422_2325
    );
    assert_eq!(
        rslip_fnv1a64(0xcbf2_9ce4_8422_2325, b"hello"),
        0xa430_d846_80aa_bd0b
    );
    assert_ne!(
        rslip_fnv1a64(0xcbf2_9ce4_8422_2325, b"a"),
        rslip_fnv1a64(0xcbf2_9ce4_8422_2325, b"b")
    );
}
