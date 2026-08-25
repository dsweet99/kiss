use super::*;
use crate::rpytest_runner::PytestRunner;
use std::cell::Cell;
use std::fs;
use std::rc::Rc;

#[test]
fn batch_runtime_publication_error_is_reported_for_miss_group() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let req = rslip_sample_request(tmp.path());
    fs::create_dir_all(&req.cache_root).unwrap();
    fs::write(req.cache_root.join("runtime"), b"not a directory").unwrap();
    let calls = Rc::new(Cell::new(0));
    let calls_for_runner = Rc::clone(&calls);
    let rslip = Rslip::new(PytestRunner::from_bounded_fn(move |_reqs, _jobs| {
        calls_for_runner.set(calls_for_runner.get() + 1);
        Vec::new()
    }));

    let outcomes = rslip.run_or_reuse_many_bounded(vec![req.clone(), req], 1);

    assert!(matches!(&outcomes[0], Err(RslipError::Io(_))));
    assert!(matches!(&outcomes[1], Err(RslipError::Io(_))));
    assert_eq!(calls.get(), 0);
}

#[test]
fn batch_runner_preparation_error_is_reported_for_miss_group() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let req = rslip_sample_request(tmp.path());
    fs::create_dir_all(&req.cache_root).unwrap();
    fs::write(req.cache_root.join("testmon"), b"not a directory").unwrap();
    let calls = Rc::new(Cell::new(0));
    let calls_for_runner = Rc::clone(&calls);
    let rslip = Rslip::new(PytestRunner::from_bounded_fn(move |_reqs, _jobs| {
        calls_for_runner.set(calls_for_runner.get() + 1);
        Vec::new()
    }));

    let outcomes = rslip.run_or_reuse_many_bounded(vec![req.clone(), req], 1);

    assert!(matches!(&outcomes[0], Err(RslipError::Io(_))));
    assert!(matches!(&outcomes[1], Err(RslipError::Io(_))));
    assert_eq!(calls.get(), 0);
}
