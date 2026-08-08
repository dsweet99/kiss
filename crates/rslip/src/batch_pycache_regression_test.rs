use super::*;
use rpytest_runner::forkserver_pytest_runner;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn miss_run_ignores_stale_same_size_pyc_for_rewritten_source() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::write(root.join("lib.py"), "def value():\n    return 1\n").unwrap();
    let test_path = root.join("test_lib.py");
    fs::write(
        &test_path,
        "from lib import value\n\ndef test_value():\n    assert value() == 2\n",
    )
    .unwrap();

    let python = python();
    let mut req = RslipRequest {
        nodeid: "test_lib.py::test_value".to_string(),
        cwd: root.to_path_buf(),
        source_root: root.to_path_buf(),
        python_version: python_version(&python),
        python,
        pytest_version: "8.0.0".to_string(),
        pytest_args: vec!["-q".to_string()],
        env: BTreeMap::new(),
        cache_root: root.join(".rslip_cache"),
        force_rerun: false,
timeout: None,
        content_fingerprint: None,
    };
    let rslip = Rslip::new(forkserver_pytest_runner());

    let failed = rslip.run_or_reuse(req.clone()).unwrap();
    assert_eq!(failed.status, TestStatus::Failed);
    let stale_pycs = snapshot_test_pycs(root);
    assert!(
        !stale_pycs.is_empty(),
        "failing run should leave pytest bytecode to poison a same-size rewrite"
    );

    fs::write(
        &test_path,
        "from lib import value\n\ndef test_value():\n    assert value() == 1\n",
    )
    .unwrap();
    // Reinstall the failing-run bytecode after the source rewrite so a naive
    // import would still execute assert value() == 2.
    restore_test_pycs(root, &stale_pycs);

    req.force_rerun = true;
    let fixed = rslip.run_or_reuse(req).unwrap();
    assert_eq!(
        fixed.status,
        TestStatus::Passed,
        "miss runs must purge stale bytecode before re-executing"
    );
}

fn snapshot_test_pycs(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let pycache = root.join("__pycache__");
    let Ok(entries) = fs::read_dir(&pycache) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("test_lib.") && name.ends_with(".pyc"))
        })
        .map(|path| {
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect()
}

fn restore_test_pycs(root: &Path, snapshot: &[(PathBuf, Vec<u8>)]) {
    let pycache = root.join("__pycache__");
    fs::create_dir_all(&pycache).unwrap();
    for (path, bytes) in snapshot {
        fs::write(path, bytes).unwrap();
    }
}
