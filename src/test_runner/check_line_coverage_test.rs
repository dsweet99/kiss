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
