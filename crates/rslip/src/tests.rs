use super::*;
use rpytest_runner::subprocess_pytest_runner;
use std::process::Command;
use std::{cell::Cell, rc::Rc};

fn python() -> PathBuf {
    std::env::var_os("PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("python"))
}

fn python_version(python: &Path) -> String {
    let output = Command::new(python)
        .arg("-c")
        .arg("import sys; print('.'.join(map(str, sys.version_info[:3])))")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "python version command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

#[test]
fn request_and_coverage_structs_expose_expected_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let req = RslipRequest::witness(tmp.path());
    assert_eq!(req.nodeid, "test_sample.py::test_ok");
    assert_eq!(req.python_version, "3.12.0");
    assert_eq!(req.pytest_version, "8.0.0");
    assert!(req.cache_root.ends_with(".rslip_cache"));

    let coverage = LineCoverage::witness();
    assert_eq!(coverage.files["app.py"], BTreeSet::from([1, 2]));

    let outcome = RslipOutcome::witness();
    assert_eq!(outcome.status, TestStatus::Passed);
    assert_eq!(outcome.cache_status, CacheStatus::Hit);
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(outcome.stdout, None);
    assert_eq!(outcome.stderr, None);
    assert_eq!(CacheStatus::witness_hit(), CacheStatus::Hit);
}

#[test]
fn validate_rslip_request_rejects_missing_cache_key_parts() {
    let tmp = tempfile::tempdir().unwrap();
    let valid = rslip_sample_request(tmp.path());
    assert!(validate_rslip_request(&valid).is_ok());

    let mut missing_nodeid = valid.clone();
    missing_nodeid.nodeid.clear();
    assert!(matches!(
        validate_rslip_request(&missing_nodeid),
        Err(RslipError::InvalidRequest(message)) if message.contains("node id")
    ));
    let mut whitespace_nodeid = valid.clone();
    whitespace_nodeid.nodeid = " \t\n".to_string();
    assert!(matches!(
        validate_rslip_request(&whitespace_nodeid),
        Err(RslipError::InvalidRequest(message)) if message.contains("node id")
    ));

    let mut missing_pytest = valid.clone();
    missing_pytest.pytest_version.clear();
    assert!(matches!(
        validate_rslip_request(&missing_pytest),
        Err(RslipError::InvalidRequest(message)) if message.contains("pytest version")
    ));

    let mut missing_python = valid;
    missing_python.python_version.clear();
    assert!(matches!(
        validate_rslip_request(&missing_python),
        Err(RslipError::InvalidRequest(message)) if message.contains("python version")
    ));
    let tmp = tempfile::tempdir().unwrap();
    let mut whitespace_versions = rslip_sample_request(tmp.path());
    whitespace_versions.pytest_version = "  ".to_string();
    whitespace_versions.python_version = "\n".to_string();
    assert!(validate_rslip_request(&whitespace_versions).is_err());
}

#[test]
fn run_or_reuse_uses_cache_on_second_call() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let calls = Rc::new(Cell::new(0));
    let runner = fake_runner(Rc::clone(&calls));
    let rslip = Rslip::new(runner);
    let req = rslip_sample_request(tmp.path());

    let first = rslip.run_or_reuse(req.clone()).unwrap();
    let second = rslip.run_or_reuse(req).unwrap();

    assert_eq!(first.cache_status, CacheStatus::MissStored);
    assert_eq!(second.cache_status, CacheStatus::Hit);
    assert_eq!(
        second.coverage.files["/project/app.py"],
        BTreeSet::from([1, 3])
    );
    assert_eq!(calls.get(), 1);
}

#[test]
fn force_rerun_skips_cache_and_returns_only_fresh_output() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let calls = Rc::new(Cell::new(0));
    let runner = fake_runner(Rc::clone(&calls));
    let rslip = Rslip::new(runner);
    let req = rslip_sample_request(tmp.path());

    let first = rslip.run_or_reuse(req.clone()).unwrap();
    let second = rslip.run_or_reuse(req.clone()).unwrap();
    let forced = rslip
        .run_or_reuse(RslipRequest {
            force_rerun: true,
            ..req
        })
        .unwrap();

    assert_eq!(first.cache_status, CacheStatus::MissStored);
    assert_eq!(first.stdout.as_deref(), Some(b"fresh stdout 1".as_slice()));
    assert_eq!(first.stderr.as_deref(), Some(b"fresh stderr 1".as_slice()));
    assert_eq!(second.cache_status, CacheStatus::Hit);
    assert_eq!(second.stdout, None);
    assert_eq!(second.stderr, None);
    assert_eq!(forced.cache_status, CacheStatus::MissStored);
    assert_eq!(forced.stdout.as_deref(), Some(b"fresh stdout 2".as_slice()));
    assert_eq!(forced.stderr.as_deref(), Some(b"fresh stderr 2".as_slice()));
    assert_eq!(calls.get(), 2);
}

#[test]
fn corrupt_cache_entry_is_treated_as_miss() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("test_sample.py"),
        "def test_ok():\n    assert True\n",
    )
    .unwrap();
    let req = rslip_sample_request(tmp.path());
    let fingerprint = rslip_cache_fingerprint(&req).unwrap();
    let path = cache::rslip_cache_entry_path(&req.cache_root, &fingerprint);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "{not json").unwrap();

    let calls = Rc::new(Cell::new(0));
    let runner = fake_runner(Rc::clone(&calls));
    let rslip = Rslip::new(runner);
    let outcome = rslip.run_or_reuse(req).unwrap();

    assert_eq!(outcome.cache_status, CacheStatus::MissStored);
    assert_eq!(calls.get(), 1);
}

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
        runner_req.preload_modules,
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

#[test]
fn subprocess_run_records_executed_lines_and_reuses_cache() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(
        tmp.path().join("app.py"),
        "def choose(flag):\n    if flag:\n        return 1\n    return 2\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("test_app.py"),
        "from app import choose\n\n\ndef test_choose_true():\n    assert choose(True) == 1\n",
    )
    .unwrap();
    let python = python();
    let req = RslipRequest {
        nodeid: "test_app.py::test_choose_true".to_string(),
        cwd: tmp.path().to_path_buf(),
        source_root: tmp.path().to_path_buf(),
        python_version: python_version(&python),
        python,
        pytest_version: "8.0.0".to_string(),
        pytest_args: vec!["-q".to_string()],
        env: BTreeMap::new(),
        cache_root: tmp.path().join(".rslip_cache"),
        force_rerun: false,
    };
    let rslip = Rslip::new(subprocess_pytest_runner());

    let first = rslip.run_or_reuse(req.clone()).unwrap();
    let second = rslip.run_or_reuse(req).unwrap();
    let app_path = tmp.path().join("app.py").canonicalize().unwrap();
    let app_key = app_path.to_string_lossy().to_string();

    assert_eq!(first.status, TestStatus::Passed);
    assert_eq!(first.cache_status, CacheStatus::MissStored);
    assert_eq!(second.cache_status, CacheStatus::Hit);
    assert!(first.coverage.files[&app_key].contains(&1));
    assert!(first.coverage.files[&app_key].contains(&2));
    assert!(first.coverage.files[&app_key].contains(&3));
    assert!(!first.coverage.files[&app_key].contains(&4));
}

fn fake_runner(calls: Rc<Cell<usize>>) -> PytestRunner {
    PytestRunner::from_fn(move |req| {
        calls.set(calls.get() + 1);
        assert_eq!(req.preload_modules, vec![runtime::MODULE_NAME.to_string()]);
        assert!(req.env.contains_key("RSLIP_COVERAGE_OUT"));
        assert!(req.env.contains_key("RSLIP_SOURCE_ROOT"));
        let path = req.artifacts[0].path.clone();
        fs::write(&path, r#"{"files":{"/project/app.py":[1,3]}}"#).unwrap();
        Ok(PytestRunOutcome {
            nodeid: req.nodeid,
            status: TestStatus::Passed,
            exit_code: Some(0),
            stdout: format!("fresh stdout {}", calls.get()).into_bytes(),
            stderr: format!("fresh stderr {}", calls.get()).into_bytes(),
            duration: Duration::from_millis(7),
            artifacts: BTreeMap::from([(runtime::COVERAGE_ARTIFACT.to_string(), path)]),
        })
    })
}
