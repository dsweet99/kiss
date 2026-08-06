    use super::*;
    use rpytest_runner::{PytestRunOutcome, TestStatus};
    use rslip::LineCoverage;
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
        assert_eq!(summary.cache_hits, 0);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.exit_code, 1);
        assert_eq!(
            summary.failed_selectors,
            vec!["test_sample.py::test_b".to_string()]
        );
        assert_eq!(
            summary.max_passing_run_duration,
            Duration::from_millis(1)
        );
    }

    #[test]
    fn format_rslip_error_includes_context() {
        let msg = format_rslip_error(RslipError::InvalidRequest("bad selector".to_string()));

        assert!(msg.contains("error: kiss test: rslip failed"));
        assert!(msg.contains("bad selector"));
    }

    #[test]
    #[should_panic(expected = "jobs must be greater than zero")]
    fn run_rslip_selectors_rejects_zero_jobs_before_spawning() {
        let tmp = tempfile::tempdir().unwrap();

        let _ = run_rslip_selectors(tmp.path(), &[], &[], false, &[], 0);
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
        )
        .unwrap();

        assert_eq!(req.nodeid, "tests/test_app.py::test_ok");
        assert_eq!(req.cwd, tmp.path());
        assert_eq!(req.pytest_args, extra);
        assert!(req.force_rerun);
        assert_eq!(req.timeout, Some(DEFAULT_PYTEST_TIMEOUT));
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
            tmp.path(),
            &selectors,
            &[],
            false,
            &[],
            3,
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
            tmp.path().join("test_sample.py"),
            "import stateful\n\n\
def test_mutate_global():\n    stateful.VALUE = 1\n    assert stateful.VALUE == 1\n\n\
def test_global_starts_clean():\n    assert stateful.VALUE == 0\n",
        )
        .unwrap();
        let selectors = vec![
            "test_sample.py::test_mutate_global".to_string(),
            "test_sample.py::test_global_starts_clean".to_string(),
        ];

        let summary =
            run_rslip_selectors(tmp.path(), &selectors, &["-q".to_string()], true, &[], 1)
                .unwrap();

        assert_eq!(summary.total, 2);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.cache_misses, 2);
    }

    #[test]
    fn print_rslip_outcome_accepts_all_status_cache_shapes() {
        for (status, cache_status) in [
            (rpytest_runner::TestStatus::Passed, PyCacheStatus::Hit),
            (
                rpytest_runner::TestStatus::Passed,
                PyCacheStatus::MissStored,
            ),
            (rpytest_runner::TestStatus::Failed, PyCacheStatus::Hit),
            (
                rpytest_runner::TestStatus::Failed,
                PyCacheStatus::MissStored,
            ),
        ] {
            print_rslip_outcome(&RslipOutcome {
                nodeid: "tests/test_app.py::test_ok".to_string(),
                status,
                exit_code: Some(i32::from(status == rpytest_runner::TestStatus::Failed)),
                duration: Duration::from_millis(1),
                coverage: LineCoverage {
                    files: BTreeMap::new(),
                },
                cache_status,
                stdout: None,
                stderr: Some(Vec::new()),
            });
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
                tmp.path(),
                &selectors,
                &[],
                false,
                &[],
                2,
                runner,
            )
            .unwrap();
            assert_eq!(summary.cache_misses, 2);
        });
        assert!(
            miss_out.contains("kiss test: rslip prepared hits=0 misses=2"),
            "missing prepared line: {miss_out}"
        );
        assert!(
            miss_out.contains("PASSED: test_sample.py::test_a"),
            "missing miss print: {miss_out}"
        );
        assert!(
            miss_out.contains("PASSED: test_sample.py::test_b"),
            "missing miss print: {miss_out}"
        );
        assert!(
            miss_out.contains("kiss test: tests_remaining="),
            "missing tests_remaining heartbeat: {miss_out}"
        );
        assert_eq!(
            miss_out.matches("PASSED: test_sample.py::test_a").count(),
            1,
            "duplicate miss lines: {miss_out}"
        );

        let cached_runner = PytestRunner::from_fn(|_| {
            panic!("cache hits must not invoke the pytest runner");
        });
        let hit_out = capture_stdout(|| {
            let summary = run_rslip_selectors_with_runner(
                tmp.path(),
                &selectors,
                &[],
                false,
                &[],
                2,
                cached_runner,
            )
            .unwrap();
            assert_eq!(summary.cache_hits, 2);
            assert_eq!(summary.max_passing_run_duration, Duration::ZERO);
            assert!(summary.failed_selectors.is_empty());
        });
        assert!(
            hit_out.contains("PASSED (cached): test_sample.py::test_a"),
            "cache hits must print via prepare-time SelectorFinalized: {hit_out}"
        );
        assert!(
            hit_out.contains("PASSED (cached): test_sample.py::test_b"),
            "cache hits must print via prepare-time SelectorFinalized: {hit_out}"
        );
        assert_eq!(
            hit_out.matches("PASSED (cached):").count(),
            2,
            "duplicate cached lines: {hit_out}"
        );
    }
