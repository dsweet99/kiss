#[test]
fn apply_bare_identity_only_repair_reports_structured_failure() {
    let repo = super::apply_tests::persistent_bare_repair_repo();
    let build =
        super::apply_tests::bare_crate_synthetic_executable_build_at(&repo, "tests::missing_case");

    let identity_err = super::apply_identity_only_repair(
        &repo,
        &[],
        &build,
        &["tests::missing_case".into()],
        "prior-generation",
        std::collections::BTreeMap::new(),
    )
    .expect_err("identity-only repair should fail without a reusable aggregate")
    .to_string();
    assert!(
        identity_err.contains("runtime line coverage") || identity_err.contains("publication"),
        "{identity_err}"
    );
    assert!(identity_err.contains("Rust"), "{identity_err}");
}

#[test]
fn apply_bare_rerun_repair_reports_structured_failure() {
    // Empty repo (no Cargo.toml): fail closed in batch-request without cargo/llvm-cov.
    let empty = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(empty.path().join(".git")).unwrap();
    let primed = super::apply_tests::persistent_bare_repair_repo();
    let build = super::apply_tests::bare_crate_synthetic_executable_build_at(
        &primed,
        "tests::missing_case",
    );

    let rerun_err = super::apply_rerun_repair(super::RerunRepairArgs {
        repo_root: empty.path(),
        ignore: &[],
        build: &build,
        prior_generation: "prior-generation",
        rerun_selectors: vec!["tests::missing_case".into()],
        replacement_binary_ids: std::collections::BTreeSet::from(["bin".into()]),
        retained_binary_line_maps: std::collections::BTreeMap::new(),
        jobs: 1,
        caller_label: "kiss test",
    })
    .expect_err("rerun repair should fail without a cargo crate")
    .to_string();
    assert!(
        rerun_err.contains("runtime line coverage")
            || rerun_err.contains("publication")
            || rerun_err.contains("failed"),
        "{rerun_err}"
    );
    assert!(rerun_err.contains("Rust"), "{rerun_err}");
}

#[test]
fn successful_ensure_does_not_create_xdg_kiss_cov_durable() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_home = tmp.path().join("xdg-cache");
    std::fs::create_dir_all(&cache_home).unwrap();
    let _xdg =
        crate::test_runner::TestEnvVarGuard::set("XDG_CACHE_HOME", cache_home.to_str().unwrap());
    let gate = kiss::GateConfig {
        test_coverage_threshold: 0,
        orphan_detection: false,
        ..Default::default()
    };
    crate::test_runner::test_mode_fixtures::with_locked_warm_python_repo(|repo, app| {
        assert!(app.is_file());
        let required = crate::test_runner::check_line_coverage::RequiredCoverageLanguages {
            python: true,
            rust: false,
        };
        super::ensure_check_runtime_coverage(repo, required, &[], 1, &[], &gate)
            .expect("warm ensure");
        assert!(
            !cache_home.join("kiss").join("kiss-cov-durable").exists(),
            "successful ensure must not publish $XDG_CACHE_HOME/kiss/kiss-cov-durable"
        );
        crate::test_runner::check_line_coverage::load_check_runtime_coverage(
            repo,
            required,
            &[],
            &gate,
            &[],
        )
        .expect("coverage must remain loadable from ./.kiss");
    });
}
