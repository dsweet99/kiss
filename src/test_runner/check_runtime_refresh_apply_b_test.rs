// Split from check_runtime_refresh_apply_test.rs for lines_per_file.

#[test]
fn apply_identity_only_repair_on_bare_index_reports_structured_failure() {
    let tmp = tempfile::tempdir().unwrap();
    super::apply_tests::bare_crate_with_lib(&tmp);
    let build = crate::test_runner::rust_llvm_cov::build_current_rust_test_executable_index(
        tmp.path(),
        &["missing_case".into()],
        &[],
        1,
    )
    .expect("bare crate can build an executable index");
    let err = super::apply_identity_only_repair(
        tmp.path(),
        &[],
        &build,
        &["missing_case".into()],
        "prior-generation",
        std::collections::BTreeMap::new(),
    )
    .expect_err("identity-only repair should fail without a reusable aggregate");
    let rendered = err.to_string();
    assert!(
        rendered.contains("runtime line coverage") || rendered.contains("publication"),
        "{rendered}"
    );
}

#[test]
fn apply_rerun_repair_on_bare_index_reports_publication_or_execution_failure() {
    let tmp = tempfile::tempdir().unwrap();
    super::apply_tests::bare_crate_with_lib(&tmp);
    let build = crate::test_runner::rust_llvm_cov::build_current_rust_test_executable_index(
        tmp.path(),
        &["missing_case".into()],
        &[],
        1,
    )
    .expect("bare crate can build an executable index");
    let err = super::apply_rerun_repair(super::RerunRepairArgs {
        repo_root: tmp.path(),
        ignore: &[],
        build: &build,
        prior_generation: "prior-generation",
        rerun_selectors: vec!["missing_case".into()],
        replacement_binary_ids: std::collections::BTreeSet::from(["bin".into()]),
        retained_binary_line_maps: std::collections::BTreeMap::new(),
        jobs: 1,
        caller_label: "kiss test",
    })
    .expect_err("rerun repair should fail on a bare crate");
    let rendered = err.to_string();
    assert!(
        rendered.contains("runtime line coverage")
            || rendered.contains("publication")
            || rendered.contains("failed"),
        "{rendered}"
    );
}

#[test]
fn apply_repair_helpers_are_metamorphic_on_error_language_tag() {
    let tmp = tempfile::tempdir().unwrap();
    super::apply_tests::bare_crate_with_lib(&tmp);
    let build = crate::test_runner::rust_llvm_cov::build_current_rust_test_executable_index(
        tmp.path(),
        &["missing_case".into()],
        &[],
        1,
    )
    .expect("index");
    let identity_err = super::apply_identity_only_repair(
        tmp.path(),
        &[],
        &build,
        &["missing_case".into()],
        "prior-generation",
        std::collections::BTreeMap::new(),
    )
    .unwrap_err()
    .to_string();
    let rerun_err = super::apply_rerun_repair(super::RerunRepairArgs {
        repo_root: tmp.path(),
        ignore: &[],
        build: &build,
        prior_generation: "prior-generation",
        rerun_selectors: vec!["missing_case".into()],
        replacement_binary_ids: std::collections::BTreeSet::from(["bin".into()]),
        retained_binary_line_maps: std::collections::BTreeMap::new(),
        jobs: 1,
        caller_label: "kiss test",
    })
    .unwrap_err()
    .to_string();
    assert!(
        identity_err.contains("Rust") && rerun_err.contains("Rust"),
        "identity={identity_err} rerun={rerun_err}"
    );
}

#[test]
fn successful_ensure_does_not_create_xdg_kiss_cov_durable() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_home = tmp.path().join("xdg-cache");
    std::fs::create_dir_all(&cache_home).unwrap();
    let _xdg = crate::test_runner::TestEnvVarGuard::set(
        "XDG_CACHE_HOME",
        cache_home.to_str().unwrap(),
    );
    let repo_tmp = tempfile::tempdir().unwrap();
    let app = crate::test_runner::test_mode_fixtures::warm_python_covering_demo(&repo_tmp);
    assert!(app.is_file());
    let repo = repo_tmp.path();
    let required = crate::test_runner::check_line_coverage::RequiredCoverageLanguages {
        python: true,
        rust: false,
    };
    super::ensure_check_runtime_coverage(repo, required, &[], 1).expect("warm ensure");
    assert!(
        !cache_home.join("kiss").join("kiss-cov-durable").exists(),
        "successful ensure must not publish $XDG_CACHE_HOME/kiss/kiss-cov-durable"
    );
    crate::test_runner::check_line_coverage::load_check_runtime_coverage(repo, required, &[])
        .expect("coverage must remain loadable from ./.kiss");
}
