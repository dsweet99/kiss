use super::*;
use crate::rslip::batch::{PreparedRslipMisses, RslipCacheCandidate, RslipCacheCandidateGroup};
use crate::rslip::cache::{rslip_cache_fingerprint, store_rslip_cache_entry};
use crate::rpytest_runner::{PytestRunError, PytestRunOutcome, PytestRunner};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

fn write_sample_tree(root: &Path) {
    fs::write(root.join("app.py"), "x = 1\n").unwrap();
    fs::write(
        root.join("test_sample.py"),
        "def test_ok():\n    assert True\n\n\
def test_a():\n    assert True\n\n\
def test_b():\n    assert True\n\n\
def test_fail():\n    assert False\n",
    )
    .unwrap();
}

fn write_coverage_artifact(req: &crate::rpytest_runner::PytestRunRequest, lines: &str) {
    let path = &req.artifacts[0].path;
    let app = req.cwd.join("app.py");
    let payload = format!(
        r#"{{"files":{{"{}":[{}]}}}}"#,
        app.to_string_lossy().replace('\\', "/"),
        lines
    );
    fs::write(path, payload).unwrap();
}

#[test]
fn batch_mixed_hit_and_miss_preserves_order_and_skips_hit_execution() {
    let tmp = tempfile::tempdir().unwrap();
    write_sample_tree(tmp.path());
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
#[allow(non_snake_case)]
fn PreparedRslipMisses_and_RslipCacheCandidateGroup_are_test_referenced() {
    let tmp = tempfile::tempdir().unwrap();
    let req = rslip_sample_request(tmp.path());
    let candidate = RslipCacheCandidate {
        index: 0,
        req,
        fingerprint: "abc".to_string(),
        canonical_cache_root: tmp.path().to_path_buf(),
    };
    let group = RslipCacheCandidateGroup {
        indices: vec![0],
        representative: candidate,
        fingerprint: "abc".to_string(),
    };
    let prepared = PreparedRslipMisses { misses: Vec::new() };

    assert_eq!(group.indices, vec![0]);
    assert_eq!(prepared.misses.len(), 0);
}

#[test]
fn batch_all_cache_hits_does_not_call_runner() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("app.py"), "x = 1\n").unwrap();
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
        &cache::RslipCacheEntry::from_outcome(&RslipOutcome::witness(), tmp.path()),
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
fn all_hit_batch_does_not_wait_for_entry_lock() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("app.py"), "x = 1\n").unwrap();
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
        &cache::RslipCacheEntry::from_outcome(&RslipOutcome::witness(), tmp.path()),
    )
    .unwrap();
    let (locked_tx, locked_rx) = mpsc::channel();
    let root_for_lock = req.cache_root.clone();
    let fingerprint_for_lock = fingerprint.clone();
    let lock_holder = thread::spawn(move || {
        let _guard = crate::rslip::lock_rslip_cache_entry(&root_for_lock, &fingerprint_for_lock).unwrap();
        locked_tx.send(()).unwrap();
        thread::sleep(Duration::from_millis(200));
    });
    locked_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let rslip = Rslip::new(PytestRunner::from_bounded_fn(move |_reqs, _jobs| {
        panic!("all-hit batch should not invoke runner")
    }));

    let started = Instant::now();
    let outcomes = rslip.run_or_reuse_many_bounded(vec![req], 1);

    assert!(started.elapsed() < Duration::from_millis(2000));
    assert_eq!(outcomes[0].as_ref().unwrap().cache_status, CacheStatus::Hit);
    lock_holder.join().unwrap();
}

#[test]
fn batch_misses_are_submitted_once_with_unique_artifacts_and_job_bound() {
    let tmp = tempfile::tempdir().unwrap();
    write_sample_tree(tmp.path());
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
                write_coverage_artifact(&req, "1,3");
                Ok(PytestRunOutcome {
                    nodeid: req.nodeid,
                    status: TestStatus::Passed,
                    exit_code: Some(0),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    duration: Duration::from_millis(1),
                    artifacts: BTreeMap::from([(
                        runtime::COVERAGE_ARTIFACT.to_string(),
                        req.artifacts[0].path.clone(),
                    )]),
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
fn duplicate_selector_requests_in_one_batch_execute_once_and_fan_out() {
    let tmp = tempfile::tempdir().unwrap();
    write_sample_tree(tmp.path());
    let batch_calls = Rc::new(Cell::new(0));
    let observed_reqs = Rc::new(Cell::new(0));
    let batch_calls_for_runner = Rc::clone(&batch_calls);
    let observed_reqs_for_runner = Rc::clone(&observed_reqs);
    let rslip = Rslip::new(PytestRunner::from_bounded_fn(move |reqs, _jobs| {
        batch_calls_for_runner.set(batch_calls_for_runner.get() + 1);
        observed_reqs_for_runner.set(reqs.len());
        assert_eq!(reqs.len(), 1);
        reqs.into_iter()
            .map(|req| {
                write_coverage_artifact(&req, "1,3");
                Ok(PytestRunOutcome {
                    nodeid: req.nodeid,
                    status: TestStatus::Passed,
                    exit_code: Some(0),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    duration: Duration::from_millis(1),
                    artifacts: BTreeMap::from([(
                        runtime::COVERAGE_ARTIFACT.to_string(),
                        req.artifacts[0].path.clone(),
                    )]),
                })
            })
            .collect()
    }));
    let req = rslip_sample_request(tmp.path());

    let outcomes = rslip.run_or_reuse_many_bounded(vec![req.clone(), req], 2);

    assert_eq!(batch_calls.get(), 1);
    assert_eq!(observed_reqs.get(), 1);
    assert_eq!(
        outcomes[0].as_ref().unwrap().cache_status,
        CacheStatus::MissStored
    );
    assert_eq!(
        outcomes[1].as_ref().unwrap().cache_status,
        CacheStatus::MissStored
    );
    assert_eq!(outcomes[0].as_ref().unwrap(), outcomes[1].as_ref().unwrap());
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
    write_sample_tree(tmp.path());
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
    write_sample_tree(tmp.path());
    let req = RslipRequest {
        nodeid: "test_sample.py::test_fail".to_string(),
        ..rslip_sample_request(tmp.path())
    };
    let failing = Rslip::new(PytestRunner::from_fn(|req| {
        write_coverage_artifact(&req, "1");
        Ok(PytestRunOutcome {
            nodeid: req.nodeid,
            status: TestStatus::Failed,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: b"assert False".to_vec(),
            duration: Duration::from_millis(1),
            artifacts: BTreeMap::from([(
                runtime::COVERAGE_ARTIFACT.to_string(),
                req.artifacts[0].path.clone(),
            )]),
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
    let rerun = Rslip::new(PytestRunner::from_fn(move |req| {
        calls_for_runner.set(calls_for_runner.get() + 1);
        write_coverage_artifact(&req, "2");
        Ok(PytestRunOutcome {
            nodeid: req.nodeid,
            status: TestStatus::Failed,
            exit_code: Some(1),
            stdout: Vec::new(),
            stderr: b"assert False".to_vec(),
            duration: Duration::from_millis(1),
            artifacts: BTreeMap::from([(
                runtime::COVERAGE_ARTIFACT.to_string(),
                req.artifacts[0].path.clone(),
            )]),
        })
    }))
    .run_or_reuse_many_bounded(vec![req], 1);

    assert_eq!(
        rerun[0].as_ref().unwrap().cache_status,
        CacheStatus::MissStored
    );
    assert_eq!(rerun[0].as_ref().unwrap().status, TestStatus::Failed);
    assert_eq!(calls.get(), 1);
}

fn count_json_files(dir: &Path) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        })
        .count()
}

#[test]
fn repeated_forced_misses_do_not_leave_unreferenced_artifact_files() {
    let tmp = tempfile::tempdir().unwrap();
    write_sample_tree(tmp.path());
    let calls = Rc::new(Cell::new(0));
    let rslip = Rslip::new(fake_runner(Rc::clone(&calls)));
    let req = RslipRequest {
        force_rerun: true,
        ..rslip_sample_request(tmp.path())
    };
    let artifacts = req.cache_root.join("artifacts");

    for _ in 0..3 {
        let outcome = rslip.run_or_reuse(req.clone()).unwrap();
        assert_eq!(outcome.cache_status, CacheStatus::MissStored);
    }

    let leftover = count_json_files(&artifacts);
    assert_eq!(
        leftover, 0,
        "coverage artifacts are consumed into cache entries; leftover files={leftover}"
    );
    assert_eq!(calls.get(), 3);
}

fn count_dir_files(dir: &Path) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .count()
}

#[test]
fn repeated_forced_misses_do_not_leave_testmon_files() {
    let tmp = tempfile::tempdir().unwrap();
    write_sample_tree(tmp.path());
    let rslip = Rslip::new(PytestRunner::from_fn(|req| {
        write_coverage_artifact(&req, "1,3");
        if let Some(path) = req.env.get("TESTMON_DATAFILE") {
            let path = PathBuf::from(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, b"x").unwrap();
        }
        Ok(PytestRunOutcome {
            nodeid: req.nodeid,
            status: TestStatus::Passed,
            exit_code: Some(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
            duration: Duration::from_millis(1),
            artifacts: BTreeMap::from([(
                runtime::COVERAGE_ARTIFACT.to_string(),
                req.artifacts[0].path.clone(),
            )]),
        })
    }));
    let req = RslipRequest {
        force_rerun: true,
        ..rslip_sample_request(tmp.path())
    };
    let testmon = req.cache_root.join("testmon");
    for _ in 0..3 {
        rslip.run_or_reuse(req.clone()).unwrap();
    }
    let leftover = count_dir_files(&testmon);
    assert_eq!(
        leftover, 0,
        "testmon files are per-miss scratch; leftover files={leftover}"
    );
}
