use super::*;

use crate::rpytest_runner::subprocess_pytest_runner;

#[test]
fn subprocess_run_records_executed_lines_and_reuses_cache() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("app.py"),
        "def choose(flag):\n    if flag:\n        return 1\n    return 2\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("test_app.py"),
        "from app import choose\n\n\ndef test_choose_true():\n    assert choose(True) == 1\n",
    )
    .unwrap();
    let python = python();
    let req = RslipRequest {
        nodeid: "test_app.py::test_choose_true".to_string(),
        cwd: tmp.path().to_path_buf(),
        source_root: tmp.path().to_path_buf(),
        python_version: python_version(&python),
        python,
        pytest_version: "8.0.0".to_string(),
        pytest_args: vec!["-q".to_string()],
        env: BTreeMap::new(),
        cache_root: tmp.path().join(".rslip_cache"),
        force_rerun: false,
        timeout: None,
        content_fingerprint: None,
    };
    let rslip = Rslip::new(subprocess_pytest_runner());

    let first = rslip.run_or_reuse(req.clone()).unwrap();
    let second = rslip.run_or_reuse(req).unwrap();
    let app_path = tmp.path().join("app.py").canonicalize().unwrap();
    let app_key = app_path.to_string_lossy().to_string();

    assert_eq!(first.status, TestStatus::Passed);
    assert_eq!(first.cache_status, CacheStatus::MissStored);
    assert_eq!(second.cache_status, CacheStatus::Hit);
    assert!(first.coverage.files[&app_key].contains(&1));
    assert!(first.coverage.files[&app_key].contains(&2));
    assert!(first.coverage.files[&app_key].contains(&3));
    assert!(!first.coverage.files[&app_key].contains(&4));
}

#[test]
fn subprocess_run_serializes_synthetic_co_filenames_like_runtime() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_synthetic.py"),
        r#"def test_synthetic_filenames():
    exec(compile("VALUE = 1\n", "<frozen importlib._bootstrap>", "exec"), {})
    exec(compile("VALUE = 2\n", ".kiss/rslip_cache/rslip_runtime.py", "exec"), {})
"#,
    )
    .unwrap();
    let python = python();
    let req = RslipRequest {
        nodeid: "test_synthetic.py::test_synthetic_filenames".to_string(),
        cwd: tmp.path().to_path_buf(),
        source_root: tmp.path().to_path_buf(),
        python_version: python_version(&python),
        python,
        pytest_version: "8.0.0".to_string(),
        pytest_args: vec!["-q".to_string()],
        env: BTreeMap::new(),
        cache_root: tmp.path().join(".rslip_cache"),
        force_rerun: false,
        timeout: None,
        content_fingerprint: None,
    };

    let outcome = Rslip::new(subprocess_pytest_runner())
        .run_or_reuse(req)
        .unwrap();
    let synthetic_runtime_key = tmp
        .path()
        .join(".kiss")
        .join("rslip_cache")
        .join("rslip_runtime.py")
        .to_string_lossy()
        .to_string();

    assert_eq!(outcome.status, TestStatus::Passed);
    assert!(
        outcome.coverage.files.contains_key(&synthetic_runtime_key),
        "expected raw synthetic runtime path in coverage keys: {:?}",
        outcome.coverage.files.keys().collect::<Vec<_>>()
    );
    assert!(
        outcome
            .coverage
            .files
            .keys()
            .all(|file| !file.starts_with("<frozen")),
        "frozen co_filename values should be filtered before serialization: {:?}",
        outcome.coverage.files.keys().collect::<Vec<_>>()
    );
}
