use super::*;
use crate::cache::rslip_cache_fingerprint;

#[test]
fn builds_pytest_runner_request_with_runtime_env_and_artifact() {
    let tmp = tempfile::tempdir().unwrap();
    let req = rslip_sample_request(tmp.path());
    let runtime_dir = tmp.path().join("runtime");
    let artifact = tmp.path().join("coverage.json");

    let runner_req = build_pytest_runner_request(&req, &runtime_dir, &artifact);

    assert_eq!(runner_req.nodeid, req.nodeid);
    assert_eq!(runner_req.cwd, req.cwd);
    assert_eq!(runner_req.python, req.python);
    assert_eq!(runner_req.pytest_args, req.pytest_args);
    assert_eq!(
        runner_req.child_preload_modules,
        vec![runtime::MODULE_NAME.to_string()]
    );
    assert_eq!(
        runner_req.env["RSLIP_COVERAGE_OUT"],
        artifact.to_string_lossy()
    );
    assert_eq!(
        runner_req.env["RSLIP_SOURCE_ROOT"],
        tmp.path().to_string_lossy()
    );
    let testmon_datafile = PathBuf::from(&runner_req.env["TESTMON_DATAFILE"]);
    assert!(testmon_datafile.starts_with(req.cache_root.join("testmon")));
    assert_eq!(
        testmon_datafile.extension().and_then(|ext| ext.to_str()),
        Some("testmondata")
    );
    assert_eq!(
        runner_req.artifacts[0].name,
        runtime::COVERAGE_ARTIFACT.to_string()
    );
    assert_eq!(runner_req.artifacts[0].path, artifact);
}

#[test]
fn build_pytest_runner_request_prepends_runtime_to_existing_pythonpath() {
    let tmp = tempfile::tempdir().unwrap();
    let mut req = rslip_sample_request(tmp.path());
    req.env
        .insert("PYTHONPATH".to_string(), "/existing/path".to_string());
    let runtime_dir = tmp.path().join("runtime");
    let artifact = tmp.path().join("coverage.json");

    let runner_req = build_pytest_runner_request(&req, &runtime_dir, &artifact);

    assert_eq!(
        runner_req.env["PYTHONPATH"],
        format!("{}:/existing/path", runtime_dir.display())
    );
}

#[test]
fn cached_rslip_outcome_omits_output_but_keeps_status_and_coverage() {
    let outcome = RslipOutcome {
        nodeid: "test_sample.py::test_ok".to_string(),
        status: TestStatus::Passed,
        exit_code: Some(0),
        duration: Duration::from_millis(7),
        coverage: LineCoverage {
            files: BTreeMap::from([("app.py".to_string(), BTreeSet::from([1, 2]))]),
        },
        cache_status: CacheStatus::MissStored,
        stdout: Some(b"fresh".to_vec()),
        stderr: Some(b"err".to_vec()),
    };
    let cached = rslip_outcome_from_cache(cache::RslipCacheEntry::from(&outcome));

    assert_eq!(cached.nodeid, "test_sample.py::test_ok");
    assert_eq!(cached.status, TestStatus::Passed);
    assert_eq!(cached.cache_status, CacheStatus::Hit);
    assert_eq!(cached.stdout, None);
    assert_eq!(cached.stderr, None);
    assert_eq!(cached.coverage.files["app.py"], BTreeSet::from([1, 2]));
}

#[test]
fn load_cached_outcomes_many_handles_empty_and_mixed_contexts() {
    assert!(load_cached_outcomes_many(&[]).is_empty());

    let tmp = tempfile::tempdir().unwrap();
    let mut first = rslip_sample_request(tmp.path());
    first.nodeid = "test_sample.py::test_a".to_string();
    let mut second = rslip_sample_request(tmp.path());
    second.nodeid = "test_sample.py::test_b".to_string();
    second.pytest_args.push("-s".to_string());
    for req in [&first, &second] {
        let outcome = RslipOutcome {
            nodeid: req.nodeid.clone(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: LineCoverage {
                files: BTreeMap::from([("app.py".to_string(), BTreeSet::from([1]))]),
            },
            cache_status: CacheStatus::MissStored,
            stdout: None,
            stderr: None,
        };
        let fingerprint = rslip_cache_fingerprint(req).unwrap();
        cache::store_rslip_cache_entry(
            &req.cache_root,
            &fingerprint,
            &cache::RslipCacheEntry::from(&outcome),
        )
        .unwrap();
    }

    let outcomes = load_cached_outcomes_many(&[first, second]);

    assert_eq!(outcomes.len(), 2);
    assert!(
        outcomes
            .iter()
            .all(|outcome| outcome.as_ref().unwrap().is_some())
    );
}

#[test]
fn load_cached_outcomes_many_reads_current_entries_without_runner() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_a():\n    assert True\n\n\
         def test_b():\n    assert True\n",
    )
    .unwrap();
    let mut first = rslip_sample_request(tmp.path());
    first.nodeid = "test_sample.py::test_a".to_string();
    let mut second = rslip_sample_request(tmp.path());
    second.nodeid = "test_sample.py::test_b".to_string();
    for (req, line) in [(&first, 1), (&second, 3)] {
        let outcome = RslipOutcome {
            nodeid: req.nodeid.clone(),
            status: TestStatus::Passed,
            exit_code: Some(0),
            duration: Duration::from_millis(1),
            coverage: LineCoverage {
                files: BTreeMap::from([("app.py".to_string(), BTreeSet::from([line]))]),
            },
            cache_status: CacheStatus::MissStored,
            stdout: Some(b"fresh stdout must not be replayed".to_vec()),
            stderr: Some(b"fresh stderr must not be replayed".to_vec()),
        };
        let fingerprint = rslip_cache_fingerprint(req).unwrap();
        cache::store_rslip_cache_entry(
            &req.cache_root,
            &fingerprint,
            &cache::RslipCacheEntry::from(&outcome),
        )
        .unwrap();
    }

    let outcomes = load_cached_outcomes_many(&[first, second]);

    assert_eq!(outcomes.len(), 2);
    let first = outcomes[0].as_ref().unwrap().as_ref().unwrap();
    let second = outcomes[1].as_ref().unwrap().as_ref().unwrap();
    assert_eq!(first.nodeid, "test_sample.py::test_a");
    assert_eq!(second.nodeid, "test_sample.py::test_b");
    assert_eq!(first.cache_status, CacheStatus::Hit);
    assert_eq!(second.cache_status, CacheStatus::Hit);
    assert_eq!(first.stdout, None);
    assert_eq!(second.stderr, None);
    assert_eq!(first.coverage.files["app.py"], BTreeSet::from([1]));
    assert_eq!(second.coverage.files["app.py"], BTreeSet::from([3]));
}

#[test]
fn rslip_coverage_from_outcome_reads_named_artifact_and_reports_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let artifact = tmp.path().join("coverage.json");
    fs::write(&artifact, r#"{"files":{"app.py":[1,3]}}"#).unwrap();
    let mut outcome = PytestRunOutcome {
        nodeid: "test_sample.py::test_ok".to_string(),
        status: TestStatus::Passed,
        exit_code: Some(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
        duration: Duration::from_millis(1),
        artifacts: BTreeMap::from([(runtime::COVERAGE_ARTIFACT.to_string(), artifact)]),
    };

    let coverage = rslip_coverage_from_outcome(&outcome).unwrap();
    outcome.artifacts.clear();
    let missing = rslip_coverage_from_outcome(&outcome).unwrap_err();

    assert_eq!(coverage.files["app.py"], BTreeSet::from([1, 3]));
    assert!(
        matches!(missing, RslipError::MissingArtifact(name) if name == runtime::COVERAGE_ARTIFACT)
    );
}
