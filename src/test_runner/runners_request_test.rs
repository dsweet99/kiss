use tempfile::TempDir;

use super::runners::*;
use crate::test_runner::TestEnvVarGuard;

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
    assert!(
        req.cache_root
            .starts_with(tmp.path().join(".kiss/rslip_cache/hosts"))
    );
    assert_eq!(
        req.cache_root.components().count(),
        tmp.path().components().count() + 4
    );
    assert!(req.force_rerun);
}

#[test]
fn rslip_request_from_parts_tracks_pythonpath_in_cache_env() {
    let _lock = crate::cwd_test_lock::lock();
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let custom = format!("{}:src", root.display());
    let _pythonpath = TestEnvVarGuard::set("PYTHONPATH", &custom);

    let req = rslip_request_from_parts(
        tmp.path(),
        "tests/test_app.py::test_ok",
        &[],
        "3.12.1",
        "8.2.0",
        false,
    )
    .unwrap();

    assert_eq!(req.env.get("PYTHONPATH"), Some(&custom));
}

#[test]
fn rslip_request_from_parts_ignores_foreign_pythonpath() {
    let _lock = crate::cwd_test_lock::lock();
    let _pythonpath = TestEnvVarGuard::set("PYTHONPATH", "/home/dsweet/Projects/kiss");
    let tmp = TempDir::new().unwrap();

    let req = rslip_request_from_parts(
        tmp.path(),
        "tests/test_app.py::test_ok",
        &[],
        "3.12.1",
        "8.2.0",
        false,
    )
    .unwrap();

    let expected = tmp
        .path()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(req.env.get("PYTHONPATH"), Some(&expected));
}

#[test]
fn rslip_request_from_parts_defaults_unset_pythonpath_to_repo_root() {
    let _lock = crate::cwd_test_lock::lock();
    unsafe { std::env::remove_var("PYTHONPATH") };
    let tmp = TempDir::new().unwrap();

    let req = rslip_request_from_parts(
        tmp.path(),
        "tests/test_app.py::test_ok",
        &[],
        "3.12.1",
        "8.2.0",
        false,
    )
    .unwrap();

    let expected = tmp
        .path()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(req.env.get("PYTHONPATH"), Some(&expected));
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

#[cfg(unix)]
#[test]
fn rslip_request_from_parts_canonicalizes_repo_identity() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("repo");
    let link = tmp.path().join("repo-link");
    std::fs::create_dir(&repo).unwrap();
    std::os::unix::fs::symlink(&repo, &link).unwrap();

    let direct = rslip_request_from_parts(
        &repo,
        "tests/test_app.py::test_ok",
        &[],
        "3.12.1",
        "8.2.0",
        false,
    )
    .unwrap();
    let symlinked = rslip_request_from_parts(
        &link,
        "tests/test_app.py::test_ok",
        &[],
        "3.12.1",
        "8.2.0",
        false,
    )
    .unwrap();

    assert_eq!(direct.cwd, symlinked.cwd);
    assert_eq!(direct.source_root, symlinked.source_root);
    assert_eq!(direct.cache_root, symlinked.cache_root);
}
