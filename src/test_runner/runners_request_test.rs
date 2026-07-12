use tempfile::TempDir;

use super::runners::*;

#[test]
fn rslip_request_from_parts_uses_selector_and_kiss_cache() {
    let tmp = TempDir::new().unwrap();
    let req = rslip_request_from_parts(
        tmp.path(),
        "tests/test_app.py::test_ok",
        &["-q".to_string()],
        "3.12.1",
        "8.2.0",
        true,
    )
    .unwrap();

    assert_eq!(req.nodeid, "tests/test_app.py::test_ok");
    assert_eq!(req.cwd, tmp.path());
    assert_eq!(req.source_root, tmp.path());
    assert_eq!(req.pytest_args, vec!["-q"]);
    assert_eq!(req.python_version, "3.12.1");
    assert_eq!(req.pytest_version, "8.2.0");
    assert_eq!(req.cache_root, tmp.path().join(".kiss").join("rslip_cache"));
    assert!(req.force_rerun);
}

#[test]
fn rslip_request_from_parts_rejects_python_before_312() {
    let tmp = TempDir::new().unwrap();
    let err = rslip_request_from_parts(
        tmp.path(),
        "tests/test_app.py::test_ok",
        &[],
        "3.11.9",
        "8.2.0",
        false,
    )
    .unwrap_err();

    assert!(err.contains("Python 3.12+"));
}

#[test]
fn rslip_request_from_parts_accepts_python_after_312() {
    let tmp = TempDir::new().unwrap();
    let req = rslip_request_from_parts(
        tmp.path(),
        "tests/test_app.py::test_ok",
        &[],
        "3.13.0",
        "8.2.0",
        false,
    )
    .unwrap();

    assert_eq!(req.python_version, "3.13.0");
}
