use super::*;

use kiss::rpytest_runner::{PytestRunOutcome, TestStatus};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fs;
use std::rc::Rc;

fn passing_outcome(req: PytestRunRequest) -> PytestRunOutcome {
    PytestRunOutcome {
        nodeid: req.nodeid,
        status: TestStatus::Passed,
        exit_code: Some(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
        duration: Duration::from_millis(1),
        artifacts: BTreeMap::new(),
    }
}

fn sample_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_a():\n    assert True\n",
    )
    .unwrap();
    tmp
}

fn run_with(
    repo_root: &Path,
    selectors: &[String],
    jobs: usize,
    gate: &kiss::GateConfig,
    runner: &PytestRunner,
) -> Vec<SelectorExecutionRecord> {
    let mut records = Vec::new();
    run_pytest_selectors_with_runner(
        &PytestSelectorsArgs {
            repo_root,
            selectors,
            extra: &[],
            force_rerun: false,
            jobs,
            gate,
        },
        runner,
        &mut |record| records.push(record),
    )
    .unwrap();
    records
}

fn observe_jobs(repo_root: &Path, requested_jobs: usize) -> usize {
    let observed = Rc::new(Cell::new(0));
    let seen = Rc::clone(&observed);
    let runner = PytestRunner::from_bounded_fn(move |reqs, jobs| {
        seen.set(jobs);
        reqs.into_iter()
            .map(|req| Ok(passing_outcome(req)))
            .collect()
    });
    run_with(
        repo_root,
        &["test_sample.py::test_a".to_string()],
        requested_jobs,
        &kiss::GateConfig::default(),
        &runner,
    );
    observed.get()
}

#[test]
fn parallel_jobs_are_capped_only_by_num_jobs_pytest() {
    let tmp = sample_repo();
    let cfg = tmp.path().join(".kissconfig");
    for (config, requested, expected) in [
        ("[test]\nnum_jobs_pytest = 5\n", 32, 5),
        ("[test]\nnum_jobs_pytest = 16\n", 8, 8),
        ("[test]\nnum_jobs = 4\nnum_jobs_pytest = 16\n", 4, 16),
        ("[test]\nnum_jobs = 4\nnum_jobs_pytest = 16\n", 2, 2),
        ("[test]\nnum_jobs = 32\n", 32, 32),
    ] {
        fs::write(&cfg, config).unwrap();
        let _override = kiss::ConfigPathOverrideGuard::enter(Some(&cfg));
        assert_eq!(observe_jobs(tmp.path(), requested), expected, "{config}");
    }
}

#[test]
fn requests_carry_no_preload_modules_or_artifacts() {
    let tmp = sample_repo();
    let requests = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&requests);
    let runner = PytestRunner::from_fn(move |req| {
        seen.borrow_mut().push(req.clone());
        Ok(passing_outcome(req))
    });
    let records = run_with(
        tmp.path(),
        &["test_sample.py::test_a".to_string()],
        1,
        &kiss::GateConfig::default(),
        &runner,
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].status, TestStatus::Passed);
    assert_eq!(records[0].cache_record, SelectorCacheRecord::MissStored);
    let requests = requests.borrow();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].child_preload_modules.is_empty());
    assert!(requests[0].artifacts.is_empty());
    assert!(requests[0].env.contains_key("PYTHONPATH"));
}

#[test]
fn finished_tests_are_stored_as_records() {
    let tmp = sample_repo();
    let runner = PytestRunner::from_fn(|req| {
        let mut outcome = passing_outcome(req);
        if outcome.nodeid.ends_with("test_b") {
            outcome.status = TestStatus::Failed;
            outcome.exit_code = Some(1);
        }
        Ok(outcome)
    });
    let selectors = [
        "test_sample.py::test_a".to_string(),
        "test_sample.py::test_b".to_string(),
    ];
    run_with(
        tmp.path(),
        &selectors,
        2,
        &kiss::GateConfig::default(),
        &runner,
    );
    let dir = kiss::test_records::records_dir(&tmp.path().canonicalize().unwrap(), "python");
    let records = kiss::test_records::load_records(&dir);
    let statuses: Vec<(String, TestStatus)> = records
        .into_iter()
        .map(|record| (record.test_id, record.status))
        .collect();
    assert_eq!(
        statuses,
        [
            ("test_sample.py::test_a".to_string(), TestStatus::Passed),
            ("test_sample.py::test_b".to_string(), TestStatus::Failed),
        ]
    );
}

#[test]
fn zero_limit_selectors_time_out_without_invoking_the_runner() {
    let tmp = sample_repo();
    let calls = Rc::new(Cell::new(0));
    let counted = Rc::clone(&calls);
    let runner = PytestRunner::from_fn(move |req| {
        counted.set(counted.get() + 1);
        Ok(passing_outcome(req))
    });
    let gate = kiss::GateConfig {
        max_unit_test_seconds: vec![("*".into(), 0.0)],
        ..Default::default()
    };
    let records = run_with(
        tmp.path(),
        &["test_sample.py::test_a".to_string()],
        1,
        &gate,
        &runner,
    );
    assert_eq!(calls.get(), 0);
    assert_eq!(records[0].status, TestStatus::TimedOut);
    assert_eq!(records[0].exit_code, Some(124));
}

#[test]
fn runner_errors_map_to_timeout_or_failure() {
    let gate = kiss::GateConfig::default();
    let missing = finished_test(
        "t.py::a",
        Err(PytestRunError::Protocol(
            "module batch result missing".into(),
        )),
        &gate,
    );
    assert_eq!(missing.status, TestStatus::TimedOut);
    assert_eq!(missing.exit_code, 124);
    let killed = finished_test(
        "t.py::a",
        Err(PytestRunError::Timeout(Duration::from_secs(2))),
        &gate,
    );
    assert_eq!(killed.status, TestStatus::TimedOut);
    assert_eq!(killed.duration, Duration::from_secs(2));
    let broken = finished_test(
        "t.py::a",
        Err(PytestRunError::Protocol("pipe closed".into())),
        &gate,
    );
    assert_eq!(broken.status, TestStatus::Failed);
    assert!(String::from_utf8_lossy(&broken.stderr).contains("pipe closed"));
    assert!(!runner_error_is_timeout(&PytestRunError::WorkerPanic));
}

#[test]
fn per_selector_timeouts_follow_the_unit_test_limits() {
    let gate = kiss::GateConfig {
        max_unit_test_seconds: vec![
            ("tests/slow/dbs".into(), 180.0),
            ("tests/allowed".into(), 60.0),
            ("*".into(), 0.0),
        ],
        ..Default::default()
    };
    let slow = timeout_for_selector_with_gate(&gate, "tests/slow/dbs/test_vdb.py::test_bulk");
    let allowed = timeout_for_selector_with_gate(&gate, "tests/allowed/test_foo.py::test_ok");
    let banned = timeout_for_selector_with_gate(&gate, "tests/fast/test_foo.py::test_ok");
    assert_eq!(slow, Duration::from_secs(180));
    assert_eq!(allowed, Duration::from_secs(60));
    assert_eq!(banned, Duration::ZERO);
}
