use super::*;

use kiss::rpytest_runner::{PytestRunOutcome, TestStatus};
use kiss::rslip::LineCoverage;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fs;
use std::rc::Rc;
use std::time::Duration;

#[cfg(unix)]
use crate::test_runner::capture_stdout::capture_stdout;

fn assert_mixed_miss_summary(summary: &SelectorExecutionSummary) {
    assert_eq!(summary.total, 2);
    assert_eq!(summary.cache_misses, 2);
    assert_eq!(summary.cache_miss_selectors.len(), 2);
    assert_eq!(summary.cache_hits, 0);
    assert_eq!(summary.failed, 1);
    assert_eq!(summary.exit_code, 1);
    assert_eq!(
        summary.failed_selectors,
        vec!["test_sample.py::test_b".to_string()]
    );
    assert_eq!(summary.max_passing_run_duration, Duration::from_millis(1));
}

#[test]
fn format_rslip_error_includes_context() {
    let msg = format_rslip_error(RslipError::InvalidRequest("bad selector".to_string()));

    assert!(msg.contains("error: kiss test: rslip failed"));
    assert!(msg.contains("bad selector"));
}

#[test]
fn protocol_batch_missing_is_quiet_timeout() {
    assert!(rslip_protocol_is_quiet_timeout(&RslipError::Runner(
        kiss::rpytest_runner::PytestRunError::Protocol(
            "module batch result missing: JSONDecodeError('Expecting value: line 1 column 1 (char 0)')"
                .to_string(),
        )
    )));
    assert!(rslip_protocol_is_quiet_timeout(&RslipError::Runner(
        kiss::rpytest_runner::PytestRunError::Protocol("module batch timed out".to_string())
    )));
    assert!(!rslip_protocol_is_quiet_timeout(
        &RslipError::InvalidRequest("bad selector".to_string())
    ));
}

#[cfg(unix)]
#[test]
fn emit_finalized_outcomes_maps_protocol_batch_missing_to_timeout() {
    let gate = kiss::GateConfig {
        max_unit_test_seconds: vec![("*".into(), 7.0)],
        ..kiss::GateConfig::default()
    };
    let out = capture_stdout(|| {
        emit_finalized_outcomes(
            vec![(
                0,
                Err(RslipError::Runner(
                    kiss::rpytest_runner::PytestRunError::Protocol(
                        "module batch result missing: JSONDecodeError".to_string(),
                    ),
                )),
            )],
            &["mod.py::test_a".to_string()],
            &gate,
        );
    });
    assert!(
        out.contains("TIMEOUT: mod.py::test_a (7.00s)"),
        "protocol batch miss must print TIMEOUT: {out}"
    );
    assert!(!out.contains("FAIL:"));
}

#[test]
#[should_panic(expected = "jobs must be greater than zero")]
fn run_rslip_selectors_rejects_zero_jobs_before_spawning() {
    let tmp = tempfile::tempdir().unwrap();

    let _ = run_rslip_selectors(
        tmp.path(),
        &[],
        &[],
        false,
        &[],
        0,
        None,
        &kiss::GateConfig::default(),
    );
}

#[test]
fn zero_limit_selectors_timeout_without_invoking_runner() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join(".kissconfig"),
        r#"[test]
max_unit_test_seconds = [["tests/allowed", 60], ["*", 0]]
"#,
    )
    .unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_allowed():\n    assert True\n\ndef test_banned():\n    assert True\n",
    )
    .unwrap();
    let selectors = vec![
        "tests/banned/test_sample.py::test_banned".to_string(),
        "tests/allowed/test_sample.py::test_allowed".to_string(),
    ];
    let observed = Rc::new(RefCell::new(Vec::new()));
    let observed_for_runner = Rc::clone(&observed);
    let runner = PytestRunner::from_bounded_fn(move |reqs, _jobs| {
        observed_for_runner
            .borrow_mut()
            .extend(reqs.iter().map(|req| req.nodeid.clone()));
        reqs.into_iter()
            .map(|req| {
                let path = req.artifacts[0].path.clone();
                let artifact_name = req.artifacts[0].name.clone();
                fs::write(&path, r#"{"files":{}}"#).unwrap();
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
    let previous = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let gate = kiss::GateConfig {
        max_unit_test_seconds: vec![("tests/allowed".into(), 60.0), ("*".into(), 0.0)],
        ..kiss::GateConfig::default()
    };
    assert_eq!(
        timeout_for_selector_with_gate(&gate, "tests/banned/test_sample.py::test_banned"),
        Duration::ZERO
    );
    let summary = run_rslip_selectors_with_runner(
        RslipBatchArgs {
            repo_root: tmp.path(),
            selectors: &selectors,
            extra: &[],
            force_rerun: true,
            force_rerun_selectors: &[],
            jobs: 1,
            content_fingerprint: None,
            gate,
        },
        runner,
    )
    .unwrap();
    std::env::set_current_dir(previous).unwrap();
    assert_eq!(
        *observed.borrow(),
        vec!["tests/allowed/test_sample.py::test_allowed"]
    );
    assert_eq!(summary.total, 2);
    assert_eq!(summary.failed, 1);
    assert_eq!(
        summary.timed_out_selectors,
        vec!["tests/banned/test_sample.py::test_banned".to_string()]
    );
    assert_eq!(summary.exit_code, 124);
}

#[test]
fn rslip_request_and_version_contracts_are_explicit() {
    let tmp = tempfile::tempdir().unwrap();
    let extra = vec!["-q".to_string()];
    let req = rslip_request_from_parts(
        tmp.path(),
        "tests/test_app.py::test_ok",
        &extra,
        "3.12.1",
        "8.3.0",
        true,
        &kiss::GateConfig::default(),
    )
    .unwrap();

    assert_eq!(req.nodeid, "tests/test_app.py::test_ok");
    assert_eq!(req.cwd, tmp.path());
    assert_eq!(req.pytest_args, extra);
    assert!(req.force_rerun);

    assert_eq!(req.timeout, Some(Duration::from_secs(2)));
    assert!(req.content_fingerprint.is_some());
    let unscoped = rslip_request_from_parts(
        tmp.path(),
        "tests/test_app.py::test_ok",
        &[],
        "3.12.1",
        "8.3.0",
        true,
        &kiss::GateConfig::default(),
    )
    .unwrap();
    assert_ne!(req.content_fingerprint, unscoped.content_fingerprint);
    assert!(python_version_supports_rslip("3.12.0"));
    assert!(python_version_supports_rslip("4.0.0"));
    assert!(!python_version_supports_rslip("3.11.9"));
}

#[test]
fn bounded_rslip_runner_handles_empty_queue() {
    let results = Rslip::new(PytestRunner::from_fn(|_| {
        panic!("empty batch should not invoke runner")
    }))
    .run_or_reuse_many_bounded(Vec::new(), 1);

    assert!(results.is_empty());
}

#[test]
fn run_rslip_selectors_submits_misses_as_single_bounded_batch() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_a():\n    assert True\n\n\
def test_b():\n    assert False\n",
    )
    .unwrap();
    let selectors = vec![
        "test_sample.py::test_a".to_string(),
        "test_sample.py::test_b".to_string(),
    ];
    let batch_calls = Rc::new(Cell::new(0));
    let observed_jobs = Rc::new(Cell::new(0));
    let observed_nodeids = Rc::new(RefCell::new(Vec::new()));
    let batch_calls_for_runner = Rc::clone(&batch_calls);
    let observed_jobs_for_runner = Rc::clone(&observed_jobs);
    let observed_nodeids_for_runner = Rc::clone(&observed_nodeids);
    let runner = PytestRunner::from_bounded_fn(move |reqs, jobs| {
        batch_calls_for_runner.set(batch_calls_for_runner.get() + 1);
        observed_jobs_for_runner.set(jobs);
        observed_nodeids_for_runner
            .borrow_mut()
            .extend(reqs.iter().map(|req| req.nodeid.clone()));
        reqs.into_iter()
            .map(|req| {
                let path = req.artifacts[0].path.clone();
                let artifact_name = req.artifacts[0].name.clone();
                fs::write(&path, r#"{"files":{"/project/app.py":[1]}}"#).unwrap();
                let failed = req.nodeid.ends_with("test_b");
                Ok(PytestRunOutcome {
                    nodeid: req.nodeid,
                    status: if failed {
                        TestStatus::Failed
                    } else {
                        TestStatus::Passed
                    },
                    exit_code: Some(i32::from(failed)),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    duration: Duration::from_millis(1),
                    artifacts: BTreeMap::from([(artifact_name, path)]),
                })
            })
            .collect()
    });

    let summary = run_rslip_selectors_with_runner(
        RslipBatchArgs {
            repo_root: tmp.path(),
            selectors: &selectors,
            extra: &[],
            force_rerun: false,
            force_rerun_selectors: &[],
            jobs: 3,
            content_fingerprint: None,
            gate: kiss::GateConfig::default(),
        },
        runner,
    )
    .unwrap();

    assert_eq!(batch_calls.get(), 1);
    assert_eq!(observed_jobs.get(), 3);
    assert_eq!(*observed_nodeids.borrow(), selectors);
    assert_mixed_miss_summary(&summary);
}

#[cfg(target_os = "linux")]
#[test]
fn linux_run_rslip_selectors_uses_isolated_forkserver_children() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("stateful.py"), "VALUE = 0\n").unwrap();
    fs::write(
        tmp.path().join("test_mutate.py"),
        "import stateful\n\n\
def test_mutate_global():\n    stateful.VALUE = 1\n    assert stateful.VALUE == 1\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("test_clean.py"),
        "import stateful\n\n\
def test_global_starts_clean():\n    assert stateful.VALUE == 0\n",
    )
    .unwrap();
    let selectors = vec![
        "test_mutate.py::test_mutate_global".to_string(),
        "test_clean.py::test_global_starts_clean".to_string(),
    ];

    let summary = run_rslip_selectors(
        tmp.path(),
        &selectors,
        &["-q".to_string()],
        true,
        &[],
        1,
        None,
        &kiss::GateConfig::default(),
    )
    .unwrap();

    assert_eq!(summary.total, 2);
    assert_eq!(summary.failed, 0);
    assert_eq!(summary.cache_misses, 2);
}

#[cfg(unix)]
#[test]
fn print_rslip_outcome_accepts_all_status_cache_shapes() {
    for (status, cache_status) in [
        (TestStatus::Passed, PyCacheStatus::Hit),
        (TestStatus::Passed, PyCacheStatus::MissStored),
        (TestStatus::Failed, PyCacheStatus::Hit),
        (TestStatus::Failed, PyCacheStatus::MissStored),
    ] {
        let out = capture_stdout(|| {
            print_rslip_outcome(
                &RslipOutcome {
                    nodeid: "tests/test_app.py::test_ok".to_string(),
                    status,
                    exit_code: Some(i32::from(status == TestStatus::Failed)),
                    duration: Duration::from_millis(1),
                    coverage: LineCoverage {
                        files: BTreeMap::new(),
                    },
                    cache_status,
                    stdout: None,
                    stderr: Some(Vec::new()),
                },
                &kiss::GateConfig::default(),
            );
        });
        assert!(
            out.contains("tests/test_app.py::test_ok"),
            "status line must go through emit_test_progress: {out}"
        );
    }
}

#[cfg(unix)]
#[test]
fn rslip_selectors_stdout_streams_outcomes_and_tests_remaining() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("app.py"), "x = 1\n").unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_a():\n    assert True\n\n\
def test_b():\n    assert True\n",
    )
    .unwrap();
    let selectors = vec![
        "test_sample.py::test_a".to_string(),
        "test_sample.py::test_b".to_string(),
    ];
    let app_key = tmp
        .path()
        .join("app.py")
        .to_string_lossy()
        .replace('\\', "/");
    let runner = PytestRunner::from_bounded_fn(move |reqs, _jobs| {
        reqs.into_iter()
            .map(|req| {
                let path = req.artifacts[0].path.clone();
                let artifact_name = req.artifacts[0].name.clone();
                let payload = format!(r#"{{"files":{{"{app_key}":[1]}}}}"#);
                fs::write(&path, payload).unwrap();
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

    let miss_out = capture_stdout(|| {
        let summary = run_rslip_selectors_with_runner(
            RslipBatchArgs {
                repo_root: tmp.path(),
                selectors: &selectors,
                extra: &[],
                force_rerun: false,
                force_rerun_selectors: &[],
                jobs: 2,
                content_fingerprint: None,
                gate: kiss::GateConfig::default(),
            },
            runner,
        )
        .unwrap();
        assert_eq!(summary.cache_misses, 2);
    });
    assert!(
        miss_out.contains("kiss test: rslip prepared hits=0 misses=2")
            && miss_out.contains("PASS: test_sample.py::test_a")
            && miss_out.contains("PASS: test_sample.py::test_b")
            && miss_out.contains("kiss test: tests_remaining="),
        "missing stream output: {miss_out}"
    );
    assert_eq!(miss_out.matches("PASS: test_sample.py::test_a").count(), 1);

    kiss::rust_llvm_cov_runner::reset_subprocess_observer();
    let cached_runner = PytestRunner::from_fn(|_| {
        panic!("cache hits must not invoke the pytest runner");
    });
    let hit_out = capture_stdout(|| {
        let summary = run_rslip_selectors_with_runner(
            RslipBatchArgs {
                repo_root: tmp.path(),
                selectors: &selectors,
                extra: &[],
                force_rerun: false,
                force_rerun_selectors: &[],
                jobs: 2,
                content_fingerprint: None,
                gate: kiss::GateConfig::default(),
            },
            cached_runner,
        )
        .unwrap();
        assert_eq!(summary.cache_hits, 2);
        assert_eq!(summary.max_passing_run_duration, Duration::ZERO);
        assert!(summary.failed_selectors.is_empty());
        assert_eq!(
            kiss::rust_llvm_cov_runner::subprocess_observer_snapshot().pytest_invocations,
            0
        );
    });
    assert!(
        hit_out.contains("PASS (cached): test_sample.py::test_a")
            && hit_out.contains("PASS (cached): test_sample.py::test_b"),
        "cache hits must print via prepare-time SelectorFinalized: {hit_out}"
    );
    assert_eq!(hit_out.matches("PASS (cached):").count(), 2);
}

#[test]
fn rslip_progress_error_and_stderr_paths_are_covered() {
    let gate = kiss::GateConfig::default();
    handle_rslip_batch_progress(
        RslipBatchProgress::Prepared {
            cache_hits: 1,
            cache_misses: 2,
            elapsed: Duration::ZERO,
        },
        &[],
        &gate,
    );
    handle_rslip_batch_progress(
        RslipBatchProgress::CachedStatusDump {
            body: "\ncached line\n".to_string(),
        },
        &[],
        &gate,
    );
    handle_rslip_batch_progress(
        RslipBatchProgress::TestsRemaining { remaining: 4 },
        &[],
        &gate,
    );
    emit_progress_lines("\n\nprogress\n");

    let mut summary = SelectorExecutionSummary::default();
    let mut statuses = Vec::new();
    record_rslip_selector_result(
        "t.py::t",
        Err(RslipError::InvalidRequest("x".to_string())),
        &gate,
        &mut summary,
        &mut statuses,
    );
    assert_eq!(summary.failed, 1);
    record_rslip_selector_result(
        "t.py::failed",
        Ok(RslipOutcome {
            nodeid: "t.py::failed".to_string(),
            status: TestStatus::Failed,
            exit_code: Some(1),
            duration: Duration::from_millis(1),
            coverage: LineCoverage {
                files: BTreeMap::new(),
            },
            cache_status: PyCacheStatus::MissStored,
            stdout: None,
            stderr: None,
        }),
        &gate,
        &mut summary,
        &mut statuses,
    );
    assert!(
        summary
            .cache_unstored_selectors
            .contains(&"t.py::failed".to_string())
    );

    emit_finalized_outcomes(
        vec![(9, Err(RslipError::InvalidRequest("x".to_string())))],
        &[],
        &gate,
    );
    print_rslip_outcome(
        &RslipOutcome {
            nodeid: "t.py::t".to_string(),
            status: TestStatus::Failed,
            exit_code: Some(1),
            duration: Duration::from_millis(1),
            coverage: LineCoverage {
                files: BTreeMap::new(),
            },
            cache_status: PyCacheStatus::MissStored,
            stdout: None,
            stderr: Some(b"boom\n".to_vec()),
        },
        &gate,
    );
}

#[test]
fn progress_failures_are_persisted_before_the_batch_ends() {
    let tmp = tempfile::tempdir().unwrap();
    let identity = python_last_status_identity("3.12.0", "8.0.0", &[]);
    persist_rslip_progress_statuses(
        tmp.path(),
        &identity,
        &["t.py::fail".to_string()],
        &RslipBatchProgress::SelectorFinalized {
            outcomes: vec![(
                0,
                Ok(RslipOutcome {
                    nodeid: "t.py::fail".to_string(),
                    status: TestStatus::Failed,
                    exit_code: Some(1),
                    duration: Duration::from_millis(1),
                    coverage: LineCoverage {
                        files: BTreeMap::new(),
                    },
                    cache_status: PyCacheStatus::MissStored,
                    stdout: None,
                    stderr: None,
                }),
            )],
        },
    );
    assert_eq!(
        crate::test_runner::last_status::prior_failures(
            tmp.path(),
            kiss::Language::Python,
            &identity
        )
        .unwrap(),
        vec!["t.py::fail".to_string()]
    );
}

#[test]
fn progress_timeouts_are_persisted_as_failures() {
    let tmp = tempfile::tempdir().unwrap();
    let identity = python_last_status_identity("3.12.0", "8.0.0", &[]);
    persist_rslip_progress_statuses(
        tmp.path(),
        &identity,
        &["t.py::slow".to_string()],
        &RslipBatchProgress::SelectorFinalized {
            outcomes: vec![(
                0,
                Ok(RslipOutcome {
                    nodeid: "t.py::slow".to_string(),
                    status: TestStatus::TimedOut,
                    exit_code: Some(124),
                    duration: Duration::from_secs(5),
                    coverage: LineCoverage {
                        files: BTreeMap::new(),
                    },
                    cache_status: PyCacheStatus::MissStored,
                    stdout: None,
                    stderr: None,
                }),
            )],
        },
    );
    assert_eq!(
        crate::test_runner::last_status::prior_failures(
            tmp.path(),
            kiss::Language::Python,
            &identity
        )
        .unwrap(),
        vec!["t.py::slow".to_string()]
    );
}

#[cfg(unix)]
#[test]
fn timeouts_are_not_retried() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_a():\n    assert True\n",
    )
    .unwrap();
    let calls = Rc::new(Cell::new(0usize));
    let jobs_seen = Rc::new(RefCell::new(Vec::new()));
    let calls_for_runner = Rc::clone(&calls);
    let jobs_for_runner = Rc::clone(&jobs_seen);
    let runner = PytestRunner::from_bounded_fn(move |reqs, jobs| {
        jobs_for_runner.borrow_mut().push(jobs);
        let attempt = calls_for_runner.get();
        calls_for_runner.set(attempt + 1);
        reqs.into_iter()
            .map(|req| {
                let path = req.artifacts[0].path.clone();
                let artifact_name = req.artifacts[0].name.clone();
                fs::write(&path, r#"{"files":{}}"#).unwrap();
                Ok(PytestRunOutcome {
                    nodeid: req.nodeid,
                    status: if attempt == 0 {
                        TestStatus::TimedOut
                    } else {
                        TestStatus::Passed
                    },
                    exit_code: Some(if attempt == 0 { 124 } else { 0 }),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    duration: Duration::from_millis(if attempt == 0 { 5000 } else { 1 }),
                    artifacts: BTreeMap::from([(artifact_name, path)]),
                })
            })
            .collect()
    });
    let previous = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let summary = run_rslip_selectors_with_runner(
        RslipBatchArgs {
            repo_root: tmp.path(),
            selectors: &["test_sample.py::test_a".to_string()],
            extra: &[],
            force_rerun: true,
            force_rerun_selectors: &[],
            jobs: 8,
            content_fingerprint: None,
            gate: kiss::GateConfig::default(),
        },
        runner,
    )
    .unwrap();
    std::env::set_current_dir(previous).unwrap();
    assert_eq!(*jobs_seen.borrow(), vec![8]);
    assert_eq!(
        summary.timed_out_selectors,
        vec!["test_sample.py::test_a".to_string()]
    );
    assert_eq!(summary.failed, 1);
}
