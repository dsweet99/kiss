#[test]
fn refresh_guard_env_name_is_stable() {
    assert_eq!(
        super::COVERAGE_RUNTIME_REFRESH_ACTIVE_ENV,
        "KISS_COVERAGE_RUNTIME_REFRESH_ACTIVE"
    );
}

#[test]
fn coverage_refresh_error_constructors_and_display_cover_all_arms() {
    let discovery = super::CoverageRefreshError::discovery("Python", "parse failed");
    assert!(discovery.to_string().contains("test discovery"));
    let publication = super::CoverageRefreshError::publication("Rust", "write failed");
    assert!(publication.to_string().contains("publication"));
    let exec = super::CoverageRefreshError::TestExecution {
        language: "Rust",
        total: 3,
        failed: 1,
        exit_code: 1,
    };
    assert!(exec.to_string().contains("1/3"));
}

#[test]
fn ensure_check_runtime_coverage_no_languages_is_ok() {
    let tmp = tempfile::tempdir().unwrap();
    let required = crate::test_runner::check_line_coverage::RequiredCoverageLanguages {
        python: false,
        rust: false,
    };
    super::ensure_check_runtime_coverage(
        tmp.path(),
        required,
        &[],
        1,
        &[],
        &kiss::GateConfig::default(),
    )
    .unwrap();
    assert!(!tmp.path().join(".kiss").exists());
}
