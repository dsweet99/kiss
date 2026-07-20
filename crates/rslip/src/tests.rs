use super::*;
use crate::cache::rslip_cache_fingerprint;
use std::{cell::Cell, rc::Rc};

#[test]
fn validate_rslip_request_rejects_missing_cache_key_parts() {
    let tmp = tempfile::tempdir().unwrap();
    let valid = rslip_sample_request(tmp.path());
    assert!(validate_rslip_request(&valid).is_ok());

    let mut missing_nodeid = valid.clone();
    missing_nodeid.nodeid.clear();
    assert!(matches!(
        validate_rslip_request(&missing_nodeid),
        Err(RslipError::InvalidRequest(message)) if message.contains("node id")
    ));
    let mut whitespace_nodeid = valid.clone();
    whitespace_nodeid.nodeid = " \t\n".to_string();
    assert!(matches!(
        validate_rslip_request(&whitespace_nodeid),
        Err(RslipError::InvalidRequest(message)) if message.contains("node id")
    ));

    let mut missing_pytest = valid.clone();
    missing_pytest.pytest_version.clear();
    assert!(matches!(
        validate_rslip_request(&missing_pytest),
        Err(RslipError::InvalidRequest(message)) if message.contains("pytest version")
    ));

    let mut missing_python = valid;
    missing_python.python_version.clear();
    assert!(matches!(
        validate_rslip_request(&missing_python),
        Err(RslipError::InvalidRequest(message)) if message.contains("python version")
    ));
    let tmp = tempfile::tempdir().unwrap();
    let mut whitespace_versions = rslip_sample_request(tmp.path());
    whitespace_versions.pytest_version = "  ".to_string();
    whitespace_versions.python_version = "\n".to_string();
    assert!(validate_rslip_request(&whitespace_versions).is_err());
}

#[test]
fn run_or_reuse_uses_cache_on_second_call() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let calls = Rc::new(Cell::new(0));
    let runner = fake_runner(Rc::clone(&calls));
    let rslip = Rslip::new(runner);
    let req = rslip_sample_request(tmp.path());

    let first = rslip.run_or_reuse(req.clone()).unwrap();
    let second = rslip.run_or_reuse(req).unwrap();

    assert_eq!(first.cache_status, CacheStatus::MissStored);
    assert_eq!(second.cache_status, CacheStatus::Hit);
    assert_eq!(
        second.coverage.files["/project/app.py"],
        BTreeSet::from([1, 3])
    );
    assert_eq!(calls.get(), 1);
}

#[test]
fn force_rerun_skips_cache_and_returns_only_fresh_output() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let calls = Rc::new(Cell::new(0));
    let runner = fake_runner(Rc::clone(&calls));
    let rslip = Rslip::new(runner);
    let req = rslip_sample_request(tmp.path());

    let first = rslip.run_or_reuse(req.clone()).unwrap();
    let second = rslip.run_or_reuse(req.clone()).unwrap();
    let forced = rslip
        .run_or_reuse(RslipRequest {
            force_rerun: true,
            ..req
        })
        .unwrap();

    assert_eq!(first.cache_status, CacheStatus::MissStored);
    assert_eq!(first.stdout.as_deref(), Some(b"fresh stdout 1".as_slice()));
    assert_eq!(first.stderr.as_deref(), Some(b"fresh stderr 1".as_slice()));
    assert_eq!(second.cache_status, CacheStatus::Hit);
    assert_eq!(second.stdout, None);
    assert_eq!(second.stderr, None);
    assert_eq!(forced.cache_status, CacheStatus::MissStored);
    assert_eq!(forced.stdout.as_deref(), Some(b"fresh stdout 2".as_slice()));
    assert_eq!(forced.stderr.as_deref(), Some(b"fresh stderr 2".as_slice()));
    assert_eq!(calls.get(), 2);
}

#[test]
fn corrupt_cache_entry_is_treated_as_miss() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let req = rslip_sample_request(tmp.path());
    let fingerprint = rslip_cache_fingerprint(&req).unwrap();
    let path = cache::rslip_cache_entry_path(&req.cache_root, &fingerprint);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "{not json").unwrap();

    let calls = Rc::new(Cell::new(0));
    let runner = fake_runner(Rc::clone(&calls));
    let rslip = Rslip::new(runner);
    let outcome = rslip.run_or_reuse(req).unwrap();

    assert_eq!(outcome.cache_status, CacheStatus::MissStored);
    assert_eq!(calls.get(), 1);
}

#[test]
fn missing_cache_entry_is_treated_as_miss() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let calls = Rc::new(Cell::new(0));
    let runner = fake_runner(Rc::clone(&calls));
    let rslip = Rslip::new(runner);

    let outcome = rslip
        .run_or_reuse(rslip_sample_request(tmp.path()))
        .unwrap();

    assert_eq!(outcome.cache_status, CacheStatus::MissStored);
    assert_eq!(calls.get(), 1);
}

#[test]
fn force_rerun_bypasses_cache_only_for_that_request_in_mixed_batch() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_a():\n    assert True\n\n\
def test_b():\n    assert True\n",
    )
    .unwrap();
    let calls = Rc::new(Cell::new(0));
    let rslip = Rslip::new(fake_runner(Rc::clone(&calls)));
    let mut cached_req = rslip_sample_request(tmp.path());
    cached_req.nodeid = "test_sample.py::test_a".to_string();
    let mut forced_req = rslip_sample_request(tmp.path());
    forced_req.nodeid = "test_sample.py::test_b".to_string();

    rslip.run_or_reuse(cached_req.clone()).unwrap();
    rslip.run_or_reuse(forced_req.clone()).unwrap();
    forced_req.force_rerun = true;
    let outcomes = rslip.run_or_reuse_many_bounded(vec![cached_req, forced_req], 2);

    assert_eq!(outcomes[0].as_ref().unwrap().cache_status, CacheStatus::Hit);
    assert_eq!(
        outcomes[1].as_ref().unwrap().cache_status,
        CacheStatus::MissStored
    );
    assert_eq!(calls.get(), 3);
}
