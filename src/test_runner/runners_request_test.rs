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
fn rust_llvm_cov_request_from_parts_sets_default_worker_slot() {
    let tmp = TempDir::new().unwrap();
    let req = rust_llvm_cov_request_from_parts(
        tmp.path(),
        "tests::gets_value",
        &["--exact".to_string()],
        "cargo-llvm-cov 0.6.0",
        "rustc 1.88.0",
        true,
    )
    .unwrap();

    assert_eq!(req.selector, "tests::gets_value");
    assert_eq!(req.worker_slot, 0);
    assert_eq!(
        req.cache_root,
        tmp.path().join(".kiss").join("rust_llvm_cov_cache")
    );
    assert_eq!(req.test_args, vec!["--exact"]);
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

#[test]
fn rust_llvm_cov_request_from_parts_uses_selector_extra_and_kiss_cache() {
    let tmp = TempDir::new().unwrap();
    let req = rust_llvm_cov_request_from_parts(
        tmp.path(),
        "smoke_sub",
        &["--exact".to_string()],
        "cargo-llvm-cov 0.6.0",
        "rustc 1.88.0",
        true,
    )
    .unwrap();

    assert_eq!(req.selector, "smoke_sub");
    assert_eq!(req.cwd, tmp.path());
    assert_eq!(req.source_root, tmp.path());
    assert_eq!(req.cargo_args, Vec::<String>::new());
    assert_eq!(req.test_args, vec!["--exact"]);
    assert_eq!(req.llvm_cov_version, "cargo-llvm-cov 0.6.0");
    assert_eq!(req.rustc_version, "rustc 1.88.0");
    assert_eq!(
        req.cache_root,
        tmp.path().join(".kiss").join("rust_llvm_cov_cache")
    );
    assert!(req.force_rerun);
}
