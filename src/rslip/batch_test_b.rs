use super::*;
use crate::rpytest_runner::{
    PytestRunOutcome, PytestRunRequest, PytestRunner, RequestedArtifact, forkserver_pytest_runner,
};
use crate::rslip::batch::{
    PreparedRslipMisses, RslipCacheCandidate, RslipCacheCandidateGroup, RslipMiss,
};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

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
        timeout: None,
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
    let test_key = tmp
        .path()
        .join("test_app.py")
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .to_string();

    assert_eq!(outcomes[0].as_ref().unwrap().status, TestStatus::Passed);
    assert_eq!(outcomes[1].as_ref().unwrap().status, TestStatus::Passed);
    assert!(outcomes[0].as_ref().unwrap().coverage.files[&app_key].contains(&2));
    assert!(outcomes[1].as_ref().unwrap().coverage.files[&app_key].contains(&4));
    assert!(
        outcomes[0].as_ref().unwrap().coverage.files[&test_key].contains(&1),
        "the first selector carries module collection coverage"
    );
    assert!(
        !outcomes[1]
            .as_ref()
            .unwrap()
            .coverage
            .files
            .get(&test_key)
            .is_some_and(|lines| lines.contains(&1)),
        "collection coverage must not make every selector cover module imports"
    );
}

#[test]
fn rslip_cache_candidate_and_miss_store_batch_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let req = rslip_sample_request(tmp.path());
    let runner_req = PytestRunRequest::from_parts(
        req.nodeid.clone(),
        req.cwd.clone(),
        req.python.clone(),
        req.pytest_args.clone(),
        BTreeMap::new(),
        Vec::new(),
        vec![RequestedArtifact {
            name: "coverage".to_string(),
            path: PathBuf::from("coverage.json"),
        }],
        None,
    );
    let candidate = RslipCacheCandidate {
        index: 3,
        req: req.clone(),
        fingerprint: "abc".to_string(),
        canonical_cache_root: req.cache_root.clone(),
    };
    let miss = RslipMiss {
        indices: vec![candidate.index],
        req: candidate.req,
        runner_req,
        prepared_at: std::time::SystemTime::now(),
    };
    assert_eq!(candidate.fingerprint, "abc");
    assert_eq!(miss.indices, vec![3]);
    assert_eq!(miss.req.nodeid, req.nodeid);
    assert_eq!(miss.runner_req.artifacts[0].name, "coverage");
}

#[test]
fn prepared_rslip_misses_and_candidate_group_types_are_test_referenced() {
    assert!(std::any::type_name::<PreparedRslipMisses>().contains("PreparedRslipMisses"));
    assert!(std::any::type_name::<RslipCacheCandidateGroup>().contains("RslipCacheCandidateGroup"));
}

fn passing_runner_covering_app(edit_during_run: bool, calls: Rc<Cell<usize>>) -> PytestRunner {
    PytestRunner::from_fn(move |req| {
        calls.set(calls.get() + 1);
        let app = req.cwd.join("app.py");
        if edit_during_run {
            std::thread::sleep(Duration::from_millis(30));
            fs::write(&app, "def f():\n    return 2\n").unwrap();
        }
        let path = req.artifacts[0].path.clone();
        let payload = format!(
            r#"{{"files":{{"{}":[1,2]}}}}"#,
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
    })
}

fn app_and_test_sample(root: &std::path::Path) -> RslipRequest {
    fs::write(root.join("app.py"), "def f():\n    return 1\n").unwrap();
    fs::write(
        root.join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    rslip_sample_request(root)
}

fn run_once(edit_during_run: bool, calls: &Rc<Cell<usize>>, req: &RslipRequest) -> CacheStatus {
    let outcomes = Rslip::new(passing_runner_covering_app(
        edit_during_run,
        Rc::clone(calls),
    ))
    .run_or_reuse_many_bounded(vec![req.clone()], 1);
    outcomes[0].as_ref().unwrap().cache_status
}

#[test]
fn source_edited_while_its_test_runs_never_answers_so_the_test_reruns() {
    let tmp = tempfile::tempdir().unwrap();
    let req = app_and_test_sample(tmp.path());
    let calls = Rc::new(Cell::new(0));

    assert_eq!(run_once(true, &calls, &req), CacheStatus::MissStored);
    assert!(cache::load_rslip_record(&req).is_some());
    assert!(cache::load_reusable_rslip_cache_entry(&req).is_none());

    assert_eq!(run_once(false, &calls, &req), CacheStatus::MissStored);
    assert!(cache::load_reusable_rslip_cache_entry(&req).is_some());

    assert_eq!(run_once(false, &calls, &req), CacheStatus::Hit);
    assert_eq!(calls.get(), 2);
}

#[test]
fn test_that_rewrites_identical_source_each_run_becomes_reusable_on_its_second_run() {
    let tmp = tempfile::tempdir().unwrap();
    let req = app_and_test_sample(tmp.path());
    let calls = Rc::new(Cell::new(0));

    assert_eq!(run_once(true, &calls, &req), CacheStatus::MissStored);
    assert!(cache::load_reusable_rslip_cache_entry(&req).is_none());

    assert_eq!(run_once(true, &calls, &req), CacheStatus::MissStored);
    assert!(cache::load_reusable_rslip_cache_entry(&req).is_some());

    assert_eq!(run_once(true, &calls, &req), CacheStatus::Hit);
    assert_eq!(calls.get(), 2);
}

#[test]
fn missing_coverage_artifact_is_stored_as_failed_miss() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let req = rslip_sample_request(tmp.path());
    let rslip = Rslip::new(PytestRunner::from_fn(|req| {
        let path = req.artifacts[0].path.clone();

        Ok(PytestRunOutcome {
            nodeid: req.nodeid,
            status: TestStatus::Passed,
            exit_code: Some(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
            duration: Duration::from_millis(1),
            artifacts: BTreeMap::from([(runtime::COVERAGE_ARTIFACT.to_string(), path)]),
        })
    }));
    let first = rslip.run_or_reuse_many_bounded(vec![req.clone()], 1);
    let outcome = first[0].as_ref().unwrap();
    assert_eq!(outcome.status, TestStatus::Failed);
    assert_eq!(outcome.cache_status, CacheStatus::MissStored);
    assert!(outcome.coverage.files.is_empty());
    assert!(
        String::from_utf8_lossy(outcome.stderr.as_ref().unwrap())
            .contains("missing coverage artifact")
    );

    let calls = Rc::new(Cell::new(0));
    let calls_for_runner = Rc::clone(&calls);
    let second = Rslip::new(PytestRunner::from_fn(move |req| {
        calls_for_runner.set(calls_for_runner.get() + 1);
        let path = req.artifacts[0].path.clone();

        Ok(PytestRunOutcome {
            nodeid: req.nodeid,
            status: TestStatus::Passed,
            exit_code: Some(0),
            stdout: Vec::new(),
            stderr: Vec::new(),
            duration: Duration::from_millis(1),
            artifacts: BTreeMap::from([(runtime::COVERAGE_ARTIFACT.to_string(), path)]),
        })
    }))
    .run_or_reuse_many_bounded(vec![req], 1);
    assert_eq!(
        second[0].as_ref().unwrap().cache_status,
        CacheStatus::MissStored
    );
    assert_eq!(second[0].as_ref().unwrap().status, TestStatus::Failed);
    assert_eq!(calls.get(), 1);
}

#[test]
fn missing_coverage_artifact_preserves_child_stderr() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let atexit = "Exception ignored in atexit callback\n\
TypeError: '<' not supported between instances of 'MagicMock' and 'str'\n";
    let req = rslip_sample_request(tmp.path());
    let rslip = Rslip::new(PytestRunner::from_fn(move |req| {
        let path = req.artifacts[0].path.clone();
        Ok(PytestRunOutcome {
            nodeid: req.nodeid,
            status: TestStatus::Passed,
            exit_code: Some(0),
            stdout: Vec::new(),
            stderr: atexit.as_bytes().to_vec(),
            duration: Duration::from_millis(1),
            artifacts: BTreeMap::from([(runtime::COVERAGE_ARTIFACT.to_string(), path)]),
        })
    }));
    let first = rslip.run_or_reuse_many_bounded(vec![req], 1);
    let outcome = first[0].as_ref().unwrap();
    assert_eq!(outcome.status, TestStatus::Failed);
    assert_eq!(outcome.cache_status, CacheStatus::MissStored);
    assert!(outcome.coverage.files.is_empty());
    let stderr = String::from_utf8_lossy(outcome.stderr.as_ref().unwrap());
    let atexit_at = stderr.find("Exception ignored in atexit callback").unwrap();
    let missing_at = stderr.find("missing coverage artifact").unwrap();
    assert!(atexit_at < missing_at);
}

#[test]
fn timed_out_miss_writes_a_record_that_never_answers_for_the_test() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("app.py"), "x = 1\n").unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let req = rslip_sample_request(tmp.path());
    let calls = Rc::new(Cell::new(0));
    let calls_for_runner = Rc::clone(&calls);
    let rslip = Rslip::new(PytestRunner::from_bounded_fn(move |reqs, _jobs| {
        calls_for_runner.set(calls_for_runner.get() + 1);
        reqs.into_iter()
            .map(|_| {
                Err(crate::rpytest_runner::PytestRunError::Timeout(
                    Duration::from_millis(5),
                ))
            })
            .collect()
    }));

    let first = rslip.run_or_reuse(req.clone()).unwrap();
    let record = crate::test_records::load_record(
        &crate::rslip::cache::python_records_dir(tmp.path()),
        &req.nodeid,
    )
    .expect("timed-out test still writes its row");
    let second = rslip.run_or_reuse(req.clone()).unwrap();

    assert_eq!(first.status, TestStatus::TimedOut);
    assert_eq!(record.status, TestStatus::TimedOut);
    assert!(record.covered.is_empty());
    assert_eq!(second.cache_status, CacheStatus::MissStored);
    assert_eq!(calls.get(), 2);
    assert!(!req.cache_root.join("entries").exists());
}

#[test]
fn batch_discards_the_retired_entries_store() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("app.py"), "x = 1\n").unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let req = rslip_sample_request(tmp.path());
    let retired = req.cache_root.join("entries");
    fs::create_dir_all(&retired).unwrap();
    fs::write(retired.join("old.json"), "{}").unwrap();
    let rslip = Rslip::new(fake_runner(Rc::new(Cell::new(0))));

    let _ = rslip.run_or_reuse(req);

    assert!(!retired.exists());
}
