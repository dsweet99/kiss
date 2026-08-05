use super::*;
use crate::batch::rslip_entry_lock_chunk_size;
use rpytest_runner::{PytestRunOutcome, PytestRunner};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::rc::Rc;
use std::time::Duration;

#[test]
fn entry_lock_chunk_size_is_capped_below_typical_nofile_soft_limit() {
    assert_eq!(rslip_entry_lock_chunk_size(1), 1);
    assert_eq!(rslip_entry_lock_chunk_size(8), 8);
    assert_eq!(rslip_entry_lock_chunk_size(256), 256);
    assert_eq!(rslip_entry_lock_chunk_size(1024), 256);
    assert_eq!(rslip_entry_lock_chunk_size(0), 1);
}

#[test]
fn large_miss_batch_chunks_entry_locks_instead_of_holding_all() {
    // Regression for EMFILE when sameq-scale miss sets held one flock FD each
    // for the entire batch (ulimit -n 4096; ~14k selectors).
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let miss_count = 4500;
    let jobs = 8;
    let batch_calls = Rc::new(Cell::new(0));
    let max_batch = Rc::new(Cell::new(0));
    let batch_calls_for_runner = Rc::clone(&batch_calls);
    let max_batch_for_runner = Rc::clone(&max_batch);
    let rslip = Rslip::new(PytestRunner::from_bounded_fn(move |reqs, jobs| {
        batch_calls_for_runner.set(batch_calls_for_runner.get() + 1);
        max_batch_for_runner.set(max_batch_for_runner.get().max(reqs.len()));
        assert!(reqs.len() <= rslip_entry_lock_chunk_size(jobs));
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
    let reqs: Vec<_> = (0..miss_count)
        .map(|i| {
            let mut req = rslip_sample_request(tmp.path());
            req.nodeid = format!("test_sample.py::test_{i}");
            req
        })
        .collect();

    let outcomes = rslip.run_or_reuse_many_bounded(reqs, jobs);

    assert_eq!(outcomes.len(), miss_count);
    assert!(outcomes.iter().all(|outcome| outcome.is_ok()));
    assert!(max_batch.get() <= rslip_entry_lock_chunk_size(jobs));
    assert!(batch_calls.get() > 1);
    assert_eq!(
        batch_calls.get(),
        miss_count.div_ceil(rslip_entry_lock_chunk_size(jobs))
    );
}
