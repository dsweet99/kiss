use super::*;
use crate::cache::{load_rslip_cache_entry, rslip_cache_fingerprint};
use rpytest_runner::{PytestRunError, PytestRunOutcome, PytestRunner};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn write_ok_sample(root: &Path) {
    fs::write(root.join("app.py"), "x = 1\n").unwrap();
    fs::write(root.join("test_sample.py"), "def test_ok():\n    assert True\n").unwrap();
}

fn numbered_requests(root: &Path, count: usize) -> Vec<RslipRequest> {
    (0..count)
        .map(|i| {
            let mut req = rslip_sample_request(root);
            req.nodeid = format!("test_sample.py::test_{i}");
            req
        })
        .collect()
}

fn ok_coverage_outcome(req: rpytest_runner::PytestRunRequest) -> Result<PytestRunOutcome, PytestRunError> {
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

#[test]
fn streaming_bounded_fn_completes_fast_indices_before_slow_first() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let order_for_runner = Arc::clone(&order);
    let runner = PytestRunner::from_streaming_bounded_fn(move |reqs, jobs, on_complete| {
        assert_eq!(jobs, 2);
        assert_eq!(reqs.len(), 3);
        let mut slots: Vec<_> = reqs.into_iter().map(Some).collect();
        // Simulate jobs=2 completing fast indices before a slow index 0.
        for index in [1usize, 2, 0] {
            let req = slots[index].take().expect("each index completes once");
            order_for_runner.lock().unwrap().push(index);
            on_complete(index, ok_coverage_outcome(req));
        }
    });
    let cwd = tempfile::tempdir().unwrap();
    let req = |nodeid: &str| {
        rpytest_runner::PytestRunRequest::from_parts(
            nodeid.to_string(),
            cwd.path().to_path_buf(),
            PathBuf::from("python"),
            Vec::new(),
            BTreeMap::new(),
            Vec::new(),
            vec![rpytest_runner::RequestedArtifact {
                name: runtime::COVERAGE_ARTIFACT.to_string(),
                path: cwd.path().join(format!("{nodeid}.json")),
            }],
            None,
        )
    };
    let got = runner.run_many_bounded(vec![req("slow"), req("fast_a"), req("fast_b")], 2);
    assert!(got.iter().all(Result::is_ok));
    assert_eq!(*order.lock().unwrap(), vec![1, 2, 0]);
    assert_eq!(got[0].as_ref().unwrap().nodeid, "slow");
    assert_eq!(got[1].as_ref().unwrap().nodeid, "fast_a");
}

#[test]
fn miss_entry_is_durable_before_later_streaming_completions() {
    let tmp = tempfile::tempdir().unwrap();
    write_ok_sample(tmp.path());
    let reqs = numbered_requests(tmp.path(), 3);
    let first_fp = rslip_cache_fingerprint(&reqs[0]).unwrap();
    let cache_root = reqs[0].cache_root.clone();
    let saw_first_on_disk = Arc::new(Mutex::new(false));
    let saw_for_runner = Arc::clone(&saw_first_on_disk);
    let rslip = Rslip::new(PytestRunner::from_streaming_bounded_fn(move |reqs, _jobs, on_complete| {
        for (index, req) in reqs.into_iter().enumerate() {
            on_complete(index, ok_coverage_outcome(req));
            if index == 0 {
                assert!(
                    load_rslip_cache_entry(&cache_root, &first_fp).is_some(),
                    "first miss must be durable before later completions"
                );
                *saw_for_runner.lock().unwrap() = true;
            }
        }
    }));
    let outcomes = rslip.run_or_reuse_many_bounded(reqs, 1);
    assert!(outcomes.iter().all(Result::is_ok));
    assert!(*saw_first_on_disk.lock().unwrap());
}

#[test]
fn selector_finalized_and_remaining_emit_before_batch_returns() {
    let tmp = tempfile::tempdir().unwrap();
    write_ok_sample(tmp.path());
    let mid_progress = Arc::new(Mutex::new(false));
    let mid_for_runner = Arc::clone(&mid_progress);
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_for_cb = Arc::clone(&events);
    let rslip = Rslip::new(PytestRunner::from_streaming_bounded_fn(move |reqs, _jobs, on_complete| {
        let total = reqs.len();
        for (index, req) in reqs.into_iter().enumerate() {
            on_complete(index, ok_coverage_outcome(req));
            if index + 1 < total {
                assert!(
                    *mid_for_runner.lock().unwrap(),
                    "SelectorFinalized must fire before later completions"
                );
            }
        }
    }));
    let outcomes = rslip.run_or_reuse_many_bounded_with_progress(
        numbered_requests(tmp.path(), 3),
        1,
        |event| {
            match &event {
                RslipBatchProgress::SelectorFinalized { .. }
                | RslipBatchProgress::CachedStatusDump { .. }
                | RslipBatchProgress::TestsRemaining { .. } => {
                    *mid_progress.lock().unwrap() = true;
                }
                RslipBatchProgress::Prepared { .. } => {}
            }
            events_for_cb.lock().unwrap().push(event);
        },
    );
    assert!(outcomes.iter().all(Result::is_ok));
    let remaining_vals: Vec<usize> = events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            RslipBatchProgress::TestsRemaining { remaining } => Some(*remaining),
            _ => None,
        })
        .collect();
    assert!(!remaining_vals.is_empty());
    assert!(remaining_vals.windows(2).all(|w| w[0] >= w[1]));
    assert_eq!(*remaining_vals.last().unwrap(), 0);
}

#[test]
fn stopped_stream_leaves_completed_misses_as_hits_on_rerun() {
    let tmp = tempfile::tempdir().unwrap();
    write_ok_sample(tmp.path());
    let reqs = numbered_requests(tmp.path(), 4);
    let stop_after = 2usize;
    let rslip = Rslip::new(PytestRunner::from_streaming_bounded_fn(move |reqs, _jobs, on_complete| {
        for (index, req) in reqs.into_iter().enumerate() {
            if index >= stop_after {
                break;
            }
            on_complete(index, ok_coverage_outcome(req));
        }
    }));
    let first = rslip.run_or_reuse_many_bounded(reqs.clone(), 1);
    assert!(first[0].is_ok());
    assert!(first[1].is_ok());
    assert!(first[2].is_err());
    assert!(first[3].is_err());

    let calls = Arc::new(Mutex::new(0usize));
    let calls_for_runner = Arc::clone(&calls);
    let rslip = Rslip::new(PytestRunner::from_bounded_fn(move |reqs, _jobs| {
        *calls_for_runner.lock().unwrap() += reqs.len();
        reqs.into_iter().map(ok_coverage_outcome).collect()
    }));
    let second = rslip.run_or_reuse_many_bounded(reqs, 1);
    assert_eq!(second[0].as_ref().unwrap().cache_status, CacheStatus::Hit);
    assert_eq!(second[1].as_ref().unwrap().cache_status, CacheStatus::Hit);
    assert_eq!(second[2].as_ref().unwrap().cache_status, CacheStatus::MissStored);
    assert_eq!(second[3].as_ref().unwrap().cache_status, CacheStatus::MissStored);
    assert_eq!(*calls.lock().unwrap(), 2);
}

#[test]
fn prepare_hits_emit_cached_dump_without_tests_remaining() {
    let tmp = tempfile::tempdir().unwrap();
    write_ok_sample(tmp.path());
    let mut hit = rslip_sample_request(tmp.path());
    hit.nodeid = "test_sample.py::test_hit".to_string();
    let fingerprint = rslip_cache_fingerprint(&hit).unwrap();
    crate::cache::store_rslip_cache_entry(
        &hit.cache_root,
        &fingerprint,
        &crate::cache::RslipCacheEntry::from_outcome(&RslipOutcome::witness(), tmp.path()),
    )
    .unwrap();
    let mut miss = rslip_sample_request(tmp.path());
    miss.nodeid = "test_sample.py::test_miss".to_string();
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_for_cb = Arc::clone(&events);
    let rslip = Rslip::new(fake_runner(std::rc::Rc::new(std::cell::Cell::new(0))));
    let _ = rslip.run_or_reuse_many_bounded_with_progress(vec![hit, miss], 1, |event| {
        events_for_cb.lock().unwrap().push(event);
    });
    let events = events.lock().unwrap();
    let prepare_dump = events.iter().position(|event| {
        matches!(event, RslipBatchProgress::CachedStatusDump { .. })
    });
    let first_remaining = events
        .iter()
        .position(|event| matches!(event, RslipBatchProgress::TestsRemaining { .. }));
    assert!(
        prepare_dump.is_some(),
        "expected CachedStatusDump for prepare hits; events={events:?}"
    );
    assert!(
        first_remaining.is_none() || prepare_dump.unwrap() < first_remaining.unwrap(),
        "cache-hit CachedStatusDump must not wait on TestsRemaining"
    );
}
