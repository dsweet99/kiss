use super::*;
use std::fs;

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
        "error: kiss cov: Rust runtime line coverage is missing population."
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
    // Regression: sameq-3 publishes population with `-p` plugin args, but kiss cov
    // previously validated with `[]`, so every warm cov refreshed then failed.
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

    let err = match load_python_runtime_coverage(tmp.path()) {
        Ok(_) => panic!(
            "empty rslip cache should fail closed, but must get past population identity"
        ),
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
