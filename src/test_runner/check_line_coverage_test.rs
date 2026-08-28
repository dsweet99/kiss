use super::*;
use std::fs;

#[test]
fn format_python_coverage_env_is_named() {
    let _ = format_python_coverage_env;
}

#[test]
fn python_coverage_classifier_skips_synthetic_and_ignored_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let app = tmp.path().join("app.py");
    let frozen = tmp.path().join("<frozen abc>");
    let runtime = tmp
        .path()
        .join(".kiss")
        .join("rslip_cache")
        .join("rslip_runtime.py");
    fs::create_dir_all(runtime.parent().unwrap()).unwrap();
    fs::write(&app, "VALUE = 1\n").unwrap();
    fs::write(&runtime, "VALUE = 2\n").unwrap();

    assert_eq!(
        classify_python_coverage_file(tmp.path(), &app.to_string_lossy()).unwrap(),
        Some("app.py".to_string())
    );
    assert_eq!(
        classify_python_coverage_file(tmp.path(), "<frozen importlib._bootstrap>").unwrap(),
        None
    );
    assert_eq!(
        classify_python_coverage_file(tmp.path(), ".kiss/rslip_cache/rslip_runtime.py").unwrap(),
        None
    );
    assert_eq!(
        classify_python_coverage_file(tmp.path(), &frozen.to_string_lossy()).unwrap(),
        None
    );
    assert_eq!(
        classify_python_coverage_file(tmp.path(), &runtime.to_string_lossy()).unwrap(),
        None
    );
}

#[test]
fn python_coverage_classifier_rejects_external_python_source() {
    let tmp = tempfile::tempdir().unwrap();
    let outside = tmp.path().parent().unwrap().join("outside.py");

    let err = classify_python_coverage_file(tmp.path(), &outside.to_string_lossy())
        .expect_err("external Python source coverage must fail closed");
    let msg = err.to_string();

    assert!(msg.contains("malformed out-of-repository path"));
    assert!(!msg.contains("kiss test commit"));
}

#[test]
fn python_coverage_classifier_rejects_relative_source_paths() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("app.py"), "VALUE = 1\n").unwrap();

    let err = classify_python_coverage_file(tmp.path(), "app.py")
        .expect_err("real rslip coverage should not contain relative source paths");
    let msg = err.to_string();

    assert!(msg.contains("malformed relative source path"));
    assert!(!msg.contains("kiss test commit"));
}

#[test]
fn missing_python_population_error_has_no_manual_refresh_instruction() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("app.py"), "VALUE = 1\n").unwrap();

    let err = load_check_runtime_coverage(
        tmp.path(),
        RequiredCoverageLanguages {
            python: true,
            rust: false,
        },
        &[],
        &kiss::GateConfig::default(),
        &[],
    )
    .expect_err("missing Python coverage should fail");
    let msg = err.to_string();

    assert!(msg.contains("Python runtime line coverage"));
    assert!(msg.contains("missing or stale/incompatible population"));
    assert!(!msg.contains("kiss test commit"));
}

#[test]
fn repository_root_for_universe_falls_back_to_canonical_universe_without_git() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();

    assert_eq!(
        repository_root_for_universe(&src),
        src.canonicalize().unwrap()
    );
}

#[test]
fn repository_root_for_universe_falls_back_to_parent_for_file_without_git() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    let file = src.join("lib.py");
    fs::create_dir_all(&src).unwrap();
    fs::write(&file, "VALUE = 1\n").unwrap();

    assert_eq!(
        repository_root_for_universe(&file),
        src.canonicalize().unwrap()
    );
}

#[test]
fn runtime_coverage_helpers_merge_lines_and_format_identities() {
    let mut target = BTreeMap::from([("a.py".to_string(), BTreeSet::from([1, 2]))]);
    let source = BTreeMap::from([
        ("a.py".to_string(), BTreeSet::from([2, 3])),
        ("b.py".to_string(), BTreeSet::from([4])),
    ]);
    merge_lines(&mut target, source);

    assert_eq!(target["a.py"], BTreeSet::from([1, 2, 3]));
    assert_eq!(target["b.py"], BTreeSet::from([4]));

    let id = backend_identity(
        "Python",
        &[("population".to_string(), "abc".to_string())],
        &target,
    );
    let repeat = backend_identity(
        "Python",
        &[("population".to_string(), "abc".to_string())],
        &target,
    );
    assert_eq!(id, repeat);
    assert_eq!(id.len(), 16);
}

#[test]
fn runtime_coverage_error_display_includes_language_and_reason() {
    let err = coverage_error("Rust", "missing population");

    assert_eq!(
        err.to_string(),
        "error: kiss test: Rust runtime line coverage is missing population."
    );
}

#[test]
fn repository_root_for_universe_walks_up_to_git_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let nested = tmp.path().join("repo/src/pkg");
    fs::create_dir_all(&nested).unwrap();
    fs::create_dir(tmp.path().join("repo/.git")).unwrap();

    assert_eq!(
        repository_root_for_universe(&nested),
        tmp.path().join("repo").canonicalize().unwrap()
    );
}

#[test]
fn load_python_runtime_coverage_matches_configured_pytest_plugin_args() {
    let _cwd = crate::cwd_test_lock::lock();
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    fs::write(
        tmp.path().join(".kissconfig"),
        "[test]\npytest_plugins = [\"pytest_asyncio.plugin\", \"random_order.plugin\"]\n",
    )
    .unwrap();
    fs::write(tmp.path().join("app.py"), "VALUE = 1\n").unwrap();
    let selector = "tests/test_app.py::test_value".to_string();
    let plugin_args = kiss::TestSectionConfig::load().pytest_plugin_cli_args();
    assert_eq!(
        plugin_args,
        vec![
            "-p".to_string(),
            "pytest_asyncio.plugin".to_string(),
            "-p".to_string(),
            "random_order.plugin".to_string(),
        ]
    );
    crate::test_runner::python_coverage_index::write_python_population_manifest_for_args(
        tmp.path(),
        std::slice::from_ref(&selector),
        &plugin_args,
    )
    .unwrap();

    let err = match load_python_runtime_coverage(
        tmp.path(),
        &plugin_args,
        &kiss::GateConfig::default(),
    ) {
        Ok(_) => {
            panic!("empty rslip cache should fail closed, but must get past population identity")
        }
        Err(err) => err,
    };
    let msg = err.to_string();
    assert!(
        !msg.contains("missing or stale/incompatible population"),
        "configured plugin args must match published population; got: {msg}"
    );
    assert!(
        msg.contains("incomplete population"),
        "expected incomplete cache after identity match; got: {msg}"
    );
}

#[test]
fn incomplete_generation_reports_problem_selectors() {
    use crate::test_runner::python_coverage_index::generation::{
        PopulationEvidence, SelectorEvidence, TimingCacheDisposition,
        population_plan_for_selectors, publish_python_population_generation,
    };
    use crate::test_runner::python_coverage_index::{
        GenerationReason, PYTHON_SELECTOR_DISCOVERY_VERSION, clear_python_generation_warm_memo,
    };
    use crate::test_runner::runners::detect_rslip_versions;
    use kiss::rpytest_runner::TestStatus;
    use std::collections::BTreeMap;
    use std::time::Duration;

    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    let Ok((py, pt)) = detect_rslip_versions(repo) else {
        return;
    };
    let selectors = vec!["t.py::ok".into(), "t.py::bad".into()];
    let mut plan = population_plan_for_selectors(repo, &selectors, &[]).unwrap();
    plan.base_identity.python_version = py;
    plan.base_identity.pytest_version = pt;
    plan.base_identity.selector_discovery_version = PYTHON_SELECTOR_DISCOVERY_VERSION.to_string();
    let mut evidence = PopulationEvidence::from_ordered_selectors(&plan.selectors);
    evidence.absorb_selector(SelectorEvidence {
        selector: "t.py::ok".into(),
        raw_status: TestStatus::Passed,
        effective_status: TestStatus::Passed,
        duration: Some(Duration::from_millis(1)),
        cache_disposition: TimingCacheDisposition::MissStored,
        reason: None,
        coverage: BTreeMap::from([("app.py".into(), [1u32].into_iter().collect())]),
    });
    evidence.absorb_selector(SelectorEvidence {
        selector: "t.py::bad".into(),
        raw_status: TestStatus::Failed,
        effective_status: TestStatus::Failed,
        duration: Some(Duration::from_millis(1)),
        cache_disposition: TimingCacheDisposition::MissStored,
        reason: Some("boom".into()),
        coverage: BTreeMap::new(),
    });
    publish_python_population_generation(repo, &plan, &evidence, GenerationReason::Complete)
        .unwrap();
    clear_python_generation_warm_memo();
    let err = load_python_runtime_coverage(repo, &[], &kiss::GateConfig::default())
        .expect_err("incomplete");
    assert_eq!(err.reason, "incomplete population");
    assert_eq!(err.problem_selectors, vec!["t.py::bad".to_string()]);
}

#[test]
fn load_rust_runtime_coverage_fails_closed_without_cache() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join(".git")).unwrap();
    let err = load_rust_runtime_coverage(tmp.path(), &[], &kiss::GateConfig::default())
        .expect_err("no rust cache");
    assert_eq!(err.language, "Rust");
}

#[test]
fn validated_cov_inputs_captures_generation_id_when_present() {
    use super::ValidatedCovInputs;
    use crate::analyze::line_coverage::RuntimeCoverageSnapshot;
    use crate::test_runner::python_coverage_index::generation::{
        PopulationEvidence, SelectorEvidence, TimingCacheDisposition,
        population_plan_for_selectors, publish_python_population_generation,
    };
    use crate::test_runner::python_coverage_index::{
        GenerationReason, PYTHON_SELECTOR_DISCOVERY_VERSION, clear_python_generation_warm_memo,
    };
    use crate::test_runner::runners::detect_rslip_versions;
    use kiss::rpytest_runner::TestStatus;
    use std::collections::BTreeMap;
    use std::time::Duration;

    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    fs::create_dir_all(repo.join(".git")).unwrap();
    fs::write(repo.join("app.py"), b"x = 1\n").unwrap();
    let Ok((py, pt)) = detect_rslip_versions(repo) else {
        return;
    };
    let selector = "t.py::test_a".to_string();
    let mut plan =
        population_plan_for_selectors(repo, std::slice::from_ref(&selector), &[]).unwrap();
    plan.base_identity.python_version = py;
    plan.base_identity.pytest_version = pt;
    plan.base_identity.selector_discovery_version = PYTHON_SELECTOR_DISCOVERY_VERSION.to_string();
    let mut evidence = PopulationEvidence::from_ordered_selectors(&plan.selectors);
    evidence.absorb_selector(SelectorEvidence {
        selector: selector.clone(),
        raw_status: TestStatus::Passed,
        effective_status: TestStatus::Passed,
        duration: Some(Duration::from_millis(1)),
        cache_disposition: TimingCacheDisposition::MissStored,
        reason: None,
        coverage: BTreeMap::from([("app.py".into(), [1u32].into_iter().collect())]),
    });
    let gen_id =
        publish_python_population_generation(repo, &plan, &evidence, GenerationReason::Complete)
            .unwrap();
    clear_python_generation_warm_memo();
    let inputs = ValidatedCovInputs::from_snapshot(
        RequiredCoverageLanguages {
            python: true,
            rust: false,
        },
        RuntimeCoverageSnapshot {
            identity: "snap".into(),
            covered_lines: BTreeMap::new(),
        },
        repo,
    );
    assert_eq!(
        inputs.python_generation_id.as_deref(),
        Some(gen_id.as_str())
    );
}

#[test]
fn load_python_runtime_coverage_honors_session_pytest_extra() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join(".git")).unwrap();
    fs::write(tmp.path().join("app.py"), "VALUE = 1\n").unwrap();
    let session_extra = vec!["--tb=short".to_string()];
    let err =
        load_python_runtime_coverage(tmp.path(), &session_extra, &kiss::GateConfig::default())
            .expect_err("no population");

    let msg = err.to_string();
    assert!(
        msg.contains("missing or stale/incompatible population")
            || msg.contains("generation identity mismatch"),
        "got: {msg}"
    );
}
