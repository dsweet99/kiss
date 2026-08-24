use super::store_completed_outcomes_with;
use crate::rust_llvm_cov_runner::test_support::{
    batch_executor_fixture_repo, batch_executor_request, witness_batch_tools,
};
use crate::rust_llvm_cov_runner::{
    RustCovCacheStatus, RustLineCoverage, RustLlvmCovOutcome, batch_fingerprint::batch_identity,
};
use crate::rpytest_runner::TestStatus;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

fn completed_outcome(selector: &str) -> RustLlvmCovOutcome {
    RustLlvmCovOutcome {
        selector: selector.to_string(),
        status: TestStatus::Passed,
        exit_code: Some(0),
        duration: Duration::from_millis(1),
        coverage: RustLineCoverage {
            files: BTreeMap::from([(
                "src/lib.rs".to_string(),
                BTreeSet::from([selector.len() as u32]),
            )]),
        },
        test_binary_ids: vec!["test-bin".to_string()],
        cache_status: RustCovCacheStatus::Hit,
        stdout: None,
        stderr: None,
    }
}

fn completed_outcomes(selectors: &[&str]) -> Vec<RustLlvmCovOutcome> {
    selectors
        .iter()
        .map(|selector| completed_outcome(selector))
        .collect()
}

fn record_active(active: &AtomicUsize, peak: &AtomicUsize) {
    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
    peak.fetch_max(now, Ordering::SeqCst);
}

#[test]
fn store_completed_outcomes_uses_bounded_parallel_workers() {
    let repo = batch_executor_fixture_repo();
    let mut req = batch_executor_request(repo.path());
    req.jobs = 3;
    req.logical_selectors = vec![
        "s0".to_string(),
        "s1".to_string(),
        "s2".to_string(),
        "s3".to_string(),
        "s4".to_string(),
    ];
    let tools = witness_batch_tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let mut completed = completed_outcomes(&["s0", "s1", "s2", "s3", "s4"]);
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(AtomicUsize::new(0));

    store_completed_outcomes_with(&req, &tools, &identity, &mut completed, {
        let active = Arc::clone(&active);
        let peak = Arc::clone(&peak);
        let started = Arc::clone(&started);
        move |_cache_root, _fingerprint, _entry| {
            record_active(&active, &peak);
            if started.fetch_add(1, Ordering::SeqCst) < 3 {
                let deadline = Instant::now() + Duration::from_millis(20);
                while started.load(Ordering::SeqCst) < 3 && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            active.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        }
    })
    .expect("parallel store succeeds");

    assert!(peak.load(Ordering::SeqCst) > 1);
    assert!(peak.load(Ordering::SeqCst) <= req.jobs);
    assert_eq!(started.load(Ordering::SeqCst), completed.len());
    assert!(
        completed
            .iter()
            .all(|outcome| outcome.cache_status == RustCovCacheStatus::MissStored)
    );
}

#[test]
fn store_completed_outcomes_jobs_one_is_serial() {
    let repo = batch_executor_fixture_repo();
    let mut req = batch_executor_request(repo.path());
    req.jobs = 1;
    req.logical_selectors = vec!["s0".to_string(), "s1".to_string(), "s2".to_string()];
    let tools = witness_batch_tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let mut completed = completed_outcomes(&["s0", "s1", "s2"]);
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let stored = Arc::new(AtomicUsize::new(0));

    store_completed_outcomes_with(&req, &tools, &identity, &mut completed, {
        let active = Arc::clone(&active);
        let peak = Arc::clone(&peak);
        let stored = Arc::clone(&stored);
        move |_cache_root, _fingerprint, _entry| {
            record_active(&active, &peak);
            stored.fetch_add(1, Ordering::SeqCst);
            active.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        }
    })
    .expect("serial store succeeds");

    assert_eq!(peak.load(Ordering::SeqCst), 1);
    assert_eq!(stored.load(Ordering::SeqCst), completed.len());
    assert!(
        completed
            .iter()
            .all(|outcome| outcome.cache_status == RustCovCacheStatus::MissStored)
    );
}

#[test]
fn store_completed_outcomes_reports_lowest_active_failure_and_stops_dispatch() {
    let repo = batch_executor_fixture_repo();
    let mut req = batch_executor_request(repo.path());
    req.jobs = 3;
    req.logical_selectors = vec![
        "s0".to_string(),
        "s1".to_string(),
        "s2".to_string(),
        "s3".to_string(),
        "s4".to_string(),
    ];
    let tools = witness_batch_tools();
    let identity = batch_identity(&req, &tools).unwrap();
    let mut completed = completed_outcomes(&["s0", "s1", "s2", "s3", "s4"]);
    let started = Arc::new(Mutex::new(Vec::new()));

    let err = store_completed_outcomes_with(&req, &tools, &identity, &mut completed, {
        let started = Arc::clone(&started);
        move |_cache_root, _fingerprint, entry| {
            started.lock().unwrap().push(entry.selector.clone());
            if entry.selector == "s0" {
                std::thread::sleep(Duration::from_millis(20));
                return Ok(());
            }
            Err(io::Error::other(format!("fail-{}", entry.selector)))
        }
    })
    .unwrap_err();

    assert!(matches!(err, crate::rust_llvm_cov_runner::RustLlvmCovError::Io(ref err) if err.to_string() == "fail-s1"));
    assert_eq!(
        completed
            .iter()
            .map(|outcome| outcome.cache_status)
            .collect::<Vec<_>>(),
        vec![
            RustCovCacheStatus::MissStored,
            RustCovCacheStatus::FreshUnstored,
            RustCovCacheStatus::FreshUnstored,
            RustCovCacheStatus::Hit,
            RustCovCacheStatus::Hit,
        ]
    );
    let mut started_selectors = started.lock().unwrap().clone();
    started_selectors.sort();
    assert_eq!(
        started_selectors,
        vec!["s0".to_string(), "s1".to_string(), "s2".to_string()]
    );
}
