use super::*;
use crate::cache::{rslip_cache_fingerprint, store_rslip_cache_entry};
use rpytest_runner::{PytestRunError, PytestRunOutcome, PytestRunner, forkserver_pytest_runner};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::rc::Rc;
use std::time::Duration;

#[test]
fn batch_mixed_hit_and_miss_preserves_order_and_skips_hit_execution() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_a():\n    assert True\n\n\
def test_b():\n    assert True\n",
    )
    .unwrap();
    let calls = Rc::new(Cell::new(0));
    let rslip = Rslip::new(fake_runner(Rc::clone(&calls)));
    let mut hit_req = rslip_sample_request(tmp.path());
    hit_req.nodeid = "test_sample.py::test_a".to_string();
    let mut miss_req = rslip_sample_request(tmp.path());
    miss_req.nodeid = "test_sample.py::test_b".to_string();

    let first = rslip.run_or_reuse(hit_req.clone()).unwrap();
    let outcomes = rslip.run_or_reuse_many_bounded(vec![hit_req, miss_req], 4);

    assert_eq!(first.cache_status, CacheStatus::MissStored);
    assert_eq!(outcomes[0].as_ref().unwrap().cache_status, CacheStatus::Hit);
    assert_eq!(
        outcomes[1].as_ref().unwrap().cache_status,
        CacheStatus::MissStored
    );
    assert_eq!(
        outcomes[0].as_ref().unwrap().nodeid,
        "test_sample.py::test_a"
    );
    assert_eq!(
        outcomes[1].as_ref().unwrap().nodeid,
        "test_sample.py::test_b"
    );
    assert_eq!(calls.get(), 2);
}

#[test]
fn batch_all_cache_hits_does_not_call_runner() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let req = rslip_sample_request(tmp.path());
    let fingerprint = rslip_cache_fingerprint(&req).unwrap();
    store_rslip_cache_entry(
        &req.cache_root,
        &fingerprint,
        &cache::RslipCacheEntry::from(&RslipOutcome::witness()),
    )
    .unwrap();
    let calls = Rc::new(Cell::new(0));
    let calls_for_runner = Rc::clone(&calls);
    let rslip = Rslip::new(PytestRunner::from_bounded_fn(move |_reqs, _jobs| {
        calls_for_runner.set(calls_for_runner.get() + 1);
        Vec::new()
    }));

    let outcomes = rslip.run_or_reuse_many_bounded(vec![req], 8);

    assert_eq!(outcomes[0].as_ref().unwrap().cache_status, CacheStatus::Hit);
    assert_eq!(calls.get(), 0);
}

#[test]
fn batch_misses_are_submitted_once_with_unique_artifacts_and_job_bound() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_a():\n    assert True\n\n\
def test_b():\n    assert True\n",
    )
    .unwrap();
    let batch_calls = Rc::new(Cell::new(0));
    let observed_jobs = Rc::new(Cell::new(0));
    let batch_calls_for_runner = Rc::clone(&batch_calls);
    let observed_jobs_for_runner = Rc::clone(&observed_jobs);
    let rslip = Rslip::new(PytestRunner::from_bounded_fn(move |reqs, jobs| {
        batch_calls_for_runner.set(batch_calls_for_runner.get() + 1);
        observed_jobs_for_runner.set(jobs);
        assert_eq!(reqs.len(), 2);
        assert_ne!(reqs[0].artifacts[0].path, reqs[1].artifacts[0].path);
        reqs.into_iter()
            .map(|req| {
                let path = req.artifacts[0].path.clone();
                fs::write(&path, r#"{"files":{"/project/app.py":[1,3]}}"#).unwrap();
                Ok(PytestRunOutcome {
                    nodeid: req.nodeid,
                    status: TestStatus::Passed,
                    exit_code: Some(0),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    duration: Duration::from_millis(1),
                    artifacts: BTreeMap::from([(runtime::COVERAGE_ARTIFACT.to_string(), path)]),
                })
            })
            .collect()
    }));
    let mut first = rslip_sample_request(tmp.path());
    first.nodeid = "test_sample.py::test_a".to_string();
    let mut second = rslip_sample_request(tmp.path());
    second.nodeid = "test_sample.py::test_b".to_string();

    let outcomes = rslip.run_or_reuse_many_bounded(vec![first, second], 7);

    assert_eq!(outcomes.len(), 2);
    assert!(outcomes.iter().all(Result::is_ok));
    assert_eq!(batch_calls.get(), 1);
    assert_eq!(observed_jobs.get(), 7);
}

#[test]
fn batch_invalid_request_returns_indexed_error_without_runner_call() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let mut invalid = rslip_sample_request(tmp.path());
    invalid.nodeid.clear();
    let calls = Rc::new(Cell::new(0));
    let calls_for_runner = Rc::clone(&calls);
    let rslip = Rslip::new(PytestRunner::from_bounded_fn(move |_reqs, _jobs| {
        calls_for_runner.set(calls_for_runner.get() + 1);
        Vec::new()
    }));

    let outcomes = rslip.run_or_reuse_many_bounded(vec![invalid], 2);

    assert!(matches!(
        &outcomes[0],
        Err(RslipError::InvalidRequest(message)) if message.contains("node id")
    ));
    assert_eq!(calls.get(), 0);
}

#[test]
fn batch_runner_failure_does_not_store_cache_entry() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let req = rslip_sample_request(tmp.path());
    let failing = Rslip::new(PytestRunner::from_fn(|_req| {
        Err(PytestRunError::Protocol("runner failed".to_string()))
    }));
    let first = failing.run_or_reuse_many_bounded(vec![req.clone()], 1);
    assert!(matches!(&first[0], Err(RslipError::Runner(_))));

    let calls = Rc::new(Cell::new(0));
    let succeeding = Rslip::new(fake_runner(Rc::clone(&calls)));
    let second = succeeding.run_or_reuse(req).unwrap();

    assert_eq!(second.cache_status, CacheStatus::MissStored);
    assert_eq!(calls.get(), 1);
}

#[test]
fn batch_pytest_failure_is_stored_after_coverage_parse() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_fail():\n    assert False\n",
    )
    .unwrap();
    let req = RslipRequest {
        nodeid: "test_sample.py::test_fail".to_string(),
        ..rslip_sample_request(tmp.path())
    };
    let failing = Rslip::new(PytestRunner::from_fn(|req| {
        let path = req.artifacts[0].path.clone();
        fs::write(&path, r#"{"files":{"/project/app.py":[1]}}"#).unwrap();
        Ok(PytestRunOutcome {
            nodeid: req.nodeid,
            status: TestStatus::Failed,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: b"assert False".to_vec(),
            duration: Duration::from_millis(1),
            artifacts: BTreeMap::from([(runtime::COVERAGE_ARTIFACT.to_string(), path)]),
        })
    }));
    let first = failing.run_or_reuse_many_bounded(vec![req.clone()], 1);
    assert_eq!(
        first[0].as_ref().unwrap().cache_status,
        CacheStatus::MissStored
    );
    assert_eq!(first[0].as_ref().unwrap().status, TestStatus::Failed);

    let calls = Rc::new(Cell::new(0));
    let calls_for_runner = Rc::clone(&calls);
    let cached = Rslip::new(PytestRunner::from_fn(move |_req| {
        calls_for_runner.set(calls_for_runner.get() + 1);
        Err(PytestRunError::Protocol(
            "unexpected runner call".to_string(),
        ))
    }))
    .run_or_reuse_many_bounded(vec![req], 1);

    assert_eq!(cached[0].as_ref().unwrap().cache_status, CacheStatus::Hit);
    assert_eq!(cached[0].as_ref().unwrap().status, TestStatus::Failed);
    assert_eq!(calls.get(), 0);
}

#[cfg(target_os = "linux")]
#[test]
fn forkserver_rslip_batch_records_coverage_via_child_system_exit() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("app.py"),
        "def choose(flag):\n    if flag:\n        return 1\n    return 2\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("test_app.py"),
        "from app import choose\n\n\
def test_true():\n    assert choose(True) == 1\n\n\
def test_false():\n    assert choose(False) == 2\n",
    )
    .unwrap();
    let python = python();
    let base_req = RslipRequest {
        nodeid: "test_app.py::test_true".to_string(),
        cwd: tmp.path().to_path_buf(),
        source_root: tmp.path().to_path_buf(),
        python_version: python_version(&python),
        python,
        pytest_version: "8.0.0".to_string(),
        pytest_args: vec!["-q".to_string()],
        env: BTreeMap::new(),
        cache_root: tmp.path().join(".rslip_cache"),
        force_rerun: false,
    };
    let mut second_req = base_req.clone();
    second_req.nodeid = "test_app.py::test_false".to_string();
    let outcomes = Rslip::new(forkserver_pytest_runner())
        .run_or_reuse_many_bounded(vec![base_req, second_req], 1);
    let app_key = tmp
        .path()
        .join("app.py")
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .to_string();

    assert_eq!(outcomes[0].as_ref().unwrap().status, TestStatus::Passed);
    assert_eq!(outcomes[1].as_ref().unwrap().status, TestStatus::Passed);
    assert!(outcomes[0].as_ref().unwrap().coverage.files[&app_key].contains(&2));
    assert!(outcomes[1].as_ref().unwrap().coverage.files[&app_key].contains(&4));
}
