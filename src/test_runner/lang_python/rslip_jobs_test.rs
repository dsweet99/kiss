use super::*;

use kiss::rpytest_runner::{PytestRunOutcome, TestStatus};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::rc::Rc;
use std::time::Duration;

fn observe_rslip_jobs(repo_root: &std::path::Path, requested_jobs: usize) -> usize {
    let observed_jobs = Rc::new(Cell::new(0));
    let observed_jobs_for_runner = Rc::clone(&observed_jobs);
    let runner = PytestRunner::from_bounded_fn(move |reqs, jobs| {
        observed_jobs_for_runner.set(jobs);
        reqs.into_iter()
            .map(|req| {
                let path = req.artifacts[0].path.clone();
                let artifact_name = req.artifacts[0].name.clone();
                fs::write(&path, r#"{"files":{"/project/app.py":[1]}}"#).unwrap();
                Ok(PytestRunOutcome {
                    nodeid: req.nodeid,
                    status: TestStatus::Passed,
                    exit_code: Some(0),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    duration: Duration::from_millis(1),
                    artifacts: BTreeMap::from([(artifact_name, path)]),
                })
            })
            .collect()
    });

    run_rslip_selectors_with_runner(
        RslipBatchArgs {
            repo_root,
            selectors: &["test_sample.py::test_a".to_string()],
            extra: &[],
            force_rerun: false,
            force_rerun_selectors: &[],
            jobs: requested_jobs,
            content_fingerprint: None,
            gate: kiss::GateConfig::default(),
        },
        runner,
    )
    .unwrap();
    observed_jobs.get()
}

#[test]
fn run_rslip_selectors_caps_parallel_jobs_to_num_jobs_pytest() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_a():\n    assert True\n",
    )
    .unwrap();
    let cfg = tmp.path().join(".kissconfig");
    fs::write(&cfg, "[test]\nnum_jobs_pytest = 5\n").unwrap();
    let _override = kiss::ConfigPathOverrideGuard::enter(Some(&cfg));

    assert_eq!(observe_rslip_jobs(tmp.path(), 32), 5);
}

#[test]
fn run_rslip_selectors_keeps_requested_jobs_at_or_below_num_jobs_pytest() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_a():\n    assert True\n",
    )
    .unwrap();
    let cfg = tmp.path().join(".kissconfig");
    fs::write(&cfg, "[test]\nnum_jobs_pytest = 16\n").unwrap();
    let _override = kiss::ConfigPathOverrideGuard::enter(Some(&cfg));

    assert_eq!(observe_rslip_jobs(tmp.path(), 8), 8);
}
