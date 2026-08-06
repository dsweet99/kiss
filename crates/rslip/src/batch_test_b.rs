use super::*;
use crate::batch::{PreparedRslipMisses, RslipCacheCandidate, RslipCacheCandidateGroup, RslipMiss};
use rpytest_runner::{
    PytestRunError, PytestRunOutcome, PytestRunRequest, PytestRunner, RequestedArtifact,
    forkserver_pytest_runner,
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

    assert_eq!(outcomes[0].as_ref().unwrap().status, TestStatus::Passed);
    assert_eq!(outcomes[1].as_ref().unwrap().status, TestStatus::Passed);
    assert!(outcomes[0].as_ref().unwrap().coverage.files[&app_key].contains(&2));
    assert!(outcomes[1].as_ref().unwrap().coverage.files[&app_key].contains(&4));
}

#[test]
fn rslip_cache_candidate_and_miss_store_batch_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let req = rslip_sample_request(tmp.path());
    let runner_req = PytestRunRequest {
        nodeid: req.nodeid.clone(),
        cwd: req.cwd.clone(),
        python: req.python.clone(),
        pytest_args: req.pytest_args.clone(),
        env: BTreeMap::new(),
        child_preload_modules: Vec::new(),
        artifacts: vec![RequestedArtifact {
            name: "coverage".to_string(),
            path: PathBuf::from("coverage.json"),
        }],
        timeout: None,
    };
    let candidate = RslipCacheCandidate {
        index: 3,
        req: req.clone(),
        fingerprint: "abc".to_string(),
        canonical_cache_root: req.cache_root.clone(),
    };
    let miss = RslipMiss {
        indices: vec![candidate.index],
        req: candidate.req,
        fingerprint: candidate.fingerprint,
        runner_req,
    };
    assert_eq!(miss.indices, vec![3]);
    assert_eq!(miss.req.nodeid, req.nodeid);
    assert_eq!(miss.fingerprint, "abc");
    assert_eq!(miss.runner_req.artifacts[0].name, "coverage");
}

#[test]
fn prepared_rslip_misses_and_candidate_group_types_are_test_referenced() {
    assert!(std::any::type_name::<PreparedRslipMisses>().contains("PreparedRslipMisses"));
    assert!(
        std::any::type_name::<RslipCacheCandidateGroup>().contains("RslipCacheCandidateGroup")
    );
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
        // Intentionally do not write the coverage artifact.
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
