use super::*;
use crate::cache::{self, rslip_cache_fingerprint, store_rslip_cache_entry};
use rpytest_runner::{PytestRunError, PytestRunOutcome, PytestRunRequest, PytestRunner};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

type TimingSlots = Arc<Mutex<Vec<Option<Instant>>>>;

#[test]
fn large_miss_batch_submits_one_bounded_call_without_retaining_entry_locks() {



    let tmp = tempfile::tempdir().unwrap();
    write_ok_sample(tmp.path());
    let miss_count = 4500;
    let jobs = 8;
    let batch_calls = Rc::new(Cell::new(0));
    let max_batch = Rc::new(Cell::new(0));
    let batch_calls_for_runner = Rc::clone(&batch_calls);
    let max_batch_for_runner = Rc::clone(&max_batch);
    let rslip = Rslip::new(PytestRunner::from_bounded_fn(move |reqs, observed_jobs| {
        batch_calls_for_runner.set(batch_calls_for_runner.get() + 1);
        max_batch_for_runner.set(max_batch_for_runner.get().max(reqs.len()));
        assert_eq!(observed_jobs, jobs);
        assert_eq!(reqs.len(), miss_count);
        reqs.into_iter().map(ok_coverage_outcome).collect()
    }));
    let reqs = numbered_sample_requests(tmp.path(), miss_count);

    let outcomes = rslip.run_or_reuse_many_bounded(reqs, jobs);

    assert_eq!(outcomes.len(), miss_count);
    assert!(outcomes.iter().all(Result::is_ok));
    assert_eq!(batch_calls.get(), 1);
    assert_eq!(max_batch.get(), miss_count);
}

#[test]
fn bounded_miss_queue_starts_third_before_slow_first_finishes() {



    let tmp = tempfile::tempdir().unwrap();
    write_ok_sample(tmp.path());
    let starts = empty_timing_slots(3);
    let ends = empty_timing_slots(3);
    let starts_for_runner = Arc::clone(&starts);
    let ends_for_runner = Arc::clone(&ends);
    let rslip = Rslip::new(PytestRunner::from_bounded_fn(move |reqs, jobs| {
        assert_eq!(reqs.len(), 3);
        assert_eq!(jobs, 2);
        run_blocking_first_worker_queue(reqs, jobs, &starts_for_runner, &ends_for_runner)
    }));

    let outcomes = rslip.run_or_reuse_many_bounded(numbered_sample_requests(tmp.path(), 3), 2);

    assert!(outcomes.iter().all(Result::is_ok));
    assert_third_started_before_first_finished(&starts, &ends);
}

#[test]
fn concurrent_cache_entry_wins_over_local_normal_outcome() {
    let tmp = tempfile::tempdir().unwrap();
    write_ok_sample(tmp.path());
    assert_concurrent_normal_entry_wins(tmp.path());
    assert_empty_concurrent_entry_does_not_win_over_timeout(tmp.path());
}

fn write_ok_sample(root: &Path) {
    fs::write(root.join("app.py"), "x = 1\n").unwrap();
    fs::write(root.join("test_sample.py"), "def test_ok():\n    assert True\n").unwrap();
}

fn numbered_sample_requests(root: &Path, count: usize) -> Vec<RslipRequest> {
    (0..count)
        .map(|i| {
            let mut req = rslip_sample_request(root);
            req.nodeid = format!("test_sample.py::test_{i}");
            req
        })
        .collect()
}

fn ok_coverage_outcome(req: PytestRunRequest) -> Result<PytestRunOutcome, PytestRunError> {
    let path = req.artifacts[0].path.clone();
    let app = req.cwd.join("app.py");
    let payload = format!(
        r#"{{"files":{{"{}":[1,3]}}}}"#,
        app.to_string_lossy().replace('\\', "/")
    );
    fs::write(&path, payload).unwrap();
    Ok(PytestRunOutcome {
        nodeid: req.nodeid,
        status: TestStatus::Passed,
        exit_code: Some(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
        duration: Duration::from_millis(1),
        artifacts: BTreeMap::from([(runtime::COVERAGE_ARTIFACT.to_string(), path)]),
    })
}

fn empty_timing_slots(len: usize) -> TimingSlots {
    Arc::new(Mutex::new(vec![None; len]))
}

fn run_blocking_first_worker_queue(
    reqs: Vec<PytestRunRequest>,
    jobs: usize,
    starts: &TimingSlots,
    ends: &TimingSlots,
) -> Vec<Result<PytestRunOutcome, PytestRunError>> {
    let len = reqs.len();
    let queue = Arc::new(Mutex::new(
        reqs.into_iter().enumerate().collect::<VecDeque<_>>(),
    ));

    let (third_started_tx, third_started_rx) = mpsc::channel::<()>();
    let third_started_rx = Arc::new(Mutex::new(Some(third_started_rx)));
    let (tx, rx) = mpsc::channel();
    for _ in 0..jobs {
        let queue = Arc::clone(&queue);
        let tx = tx.clone();
        let starts = Arc::clone(starts);
        let ends = Arc::clone(ends);
        let third_rx = Arc::clone(&third_started_rx);
        let third_tx = third_started_tx.clone();
        thread::spawn(move || {
            loop {

                let Some((index, req)) = queue.lock().unwrap().pop_front() else {
                    break;
                };
                starts.lock().unwrap()[index] = Some(Instant::now());
                if index == 0 {
                    let receiver = third_rx
                        .lock()
                        .unwrap()
                        .take()
                        .expect("first miss owns the third-started receiver");
                    receiver
                        .recv_timeout(Duration::from_secs(2))
                        .expect("third miss must start while first is still running");
                } else if index == 2 {
                    let _ = third_tx.send(());
                }
                ends.lock().unwrap()[index] = Some(Instant::now());
                tx.send((index, ok_coverage_outcome(req))).unwrap();
            }
        });
    }
    drop(tx);
    drop(third_started_tx);
    let mut out = Vec::new();
    out.resize_with(len, || Err(PytestRunError::WorkerPanic));
    for (index, result) in rx {
        out[index] = result;
    }
    out
}

fn assert_third_started_before_first_finished(starts: &TimingSlots, ends: &TimingSlots) {
    let starts = starts.lock().unwrap();
    let ends = ends.lock().unwrap();
    let third_start = starts[2].expect("third test should start");
    let first_end = ends[0].expect("first test should finish");
    assert!(
        third_start < first_end,
        "third miss must start before slow first miss finishes (continuous queue)"
    );
}

fn assert_concurrent_normal_entry_wins(root: &Path) {
    let mut req = rslip_sample_request(root);
    req.nodeid = "test_sample.py::test_normal".to_string();
    let fingerprint = rslip_cache_fingerprint(&req).unwrap();
    let cache_root = req.cache_root.clone();
    let app_key = root.join("app.py").to_string_lossy().replace('\\', "/");
    let concurrent = cache::RslipCacheEntry::from_outcome(
        &passed_coverage_outcome(&req.nodeid, &app_key, BTreeSet::from([2, 4])),
        root,
    );
    let rslip = Rslip::new(PytestRunner::from_bounded_fn(move |reqs, _jobs| {
        store_rslip_cache_entry(&cache_root, &fingerprint, &concurrent).unwrap();
        reqs.into_iter().map(ok_coverage_outcome).collect()
    }));
    let outcomes = rslip.run_or_reuse_many_bounded(vec![req], 1);
    assert_eq!(outcomes[0].as_ref().unwrap().cache_status, CacheStatus::Hit);
    assert_eq!(
        outcomes[0].as_ref().unwrap().coverage.files[&app_key],
        BTreeSet::from([2, 4])
    );
}

fn assert_empty_concurrent_entry_does_not_win_over_timeout(root: &Path) {


    let mut req = rslip_sample_request(root);
    req.nodeid = "test_sample.py::test_timeout".to_string();
    let fingerprint = rslip_cache_fingerprint(&req).unwrap();
    let cache_root = req.cache_root.clone();
    let concurrent = cache::RslipCacheEntry::from_outcome(
        &failed_empty_outcome(&req.nodeid, 7),
        root,
    );
    let rslip = Rslip::new(PytestRunner::from_bounded_fn(move |reqs, _jobs| {
        store_rslip_cache_entry(&cache_root, &fingerprint, &concurrent).unwrap();
        reqs.into_iter()
            .map(|_| Err(PytestRunError::Timeout(Duration::from_millis(50))))
            .collect()
    }));
    let outcomes = rslip.run_or_reuse_many_bounded(vec![req], 1);
    let outcome = outcomes[0].as_ref().unwrap();
    assert_eq!(outcome.cache_status, CacheStatus::MissStored);
    assert_eq!(outcome.exit_code, Some(124));
    let stderr = outcome.stderr.as_deref().unwrap_or(b"");
    assert!(
        !String::from_utf8_lossy(stderr).contains("rslip: pytest timed out"),
        "timeout must not emit the redundant rslip pytest-timeout stderr line"
    );
}

fn passed_coverage_outcome(nodeid: &str, app_key: &str, lines: BTreeSet<u32>) -> RslipOutcome {
    RslipOutcome {
        nodeid: nodeid.to_string(),
        status: TestStatus::Passed,
        exit_code: Some(0),
        duration: Duration::from_millis(9),
        coverage: LineCoverage {
            files: BTreeMap::from([(app_key.to_string(), lines)]),
        },
        cache_status: CacheStatus::MissStored,
        stdout: None,
        stderr: None,
    }
}

fn failed_empty_outcome(nodeid: &str, exit_code: i32) -> RslipOutcome {
    RslipOutcome {
        nodeid: nodeid.to_string(),
        status: TestStatus::Failed,
        exit_code: Some(exit_code),
        duration: Duration::from_millis(3),
        coverage: LineCoverage {
            files: BTreeMap::new(),
        },
        cache_status: CacheStatus::MissStored,
        stdout: None,
        stderr: Some(b"concurrent timeout entry\n".to_vec()),
    }
}
