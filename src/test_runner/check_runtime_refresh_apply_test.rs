#[test]
fn scoped_refresh_env_guard_sets_and_restores_process_env() {
    let key = super::COVERAGE_RUNTIME_REFRESH_ACTIVE_ENV;
    let previous = std::env::var_os(key);
    unsafe { std::env::remove_var(key) };
    {
        let _guard = super::ScopedRefreshEnvGuard::set();
        assert_eq!(
            std::env::var_os(key).as_deref(),
            Some(std::ffi::OsStr::new("1"))
        );
        let _nested = super::ScopedRefreshEnvGuard::set();
        assert_eq!(
            std::env::var_os(key).as_deref(),
            Some(std::ffi::OsStr::new("1"))
        );
    }
    assert!(std::env::var_os(key).is_none());
    if let Some(value) = previous {
        unsafe { std::env::set_var(key, value) };
    }
}

#[test]
fn restore_refresh_active_env_covers_set_and_clear_arms() {
    let key = super::COVERAGE_RUNTIME_REFRESH_ACTIVE_ENV;
    let previous = std::env::var_os(key);
    unsafe { std::env::set_var(key, "seed") };
    super::restore_refresh_active_env(Some(std::ffi::OsString::from("restored")));
    assert_eq!(
        std::env::var_os(key).as_deref(),
        Some(std::ffi::OsStr::new("restored"))
    );
    super::restore_refresh_active_env(None);
    assert!(std::env::var_os(key).is_none());
    match previous {
        Some(value) => unsafe { std::env::set_var(key, value) },
        None => unsafe { std::env::remove_var(key) },
    }
}

#[test]
fn restore_refresh_active_env_is_metamorphic_under_nested_clear_set() {
    let key = super::COVERAGE_RUNTIME_REFRESH_ACTIVE_ENV;
    let previous = std::env::var_os(key);
    let seed = 0xC0FFEE_u64;
    eprintln!("restore_refresh metamorphic seed={seed:#x}");
    unsafe { std::env::remove_var(key) };
    for i in 0..8 {
        let marker = format!("m{i}-{}", seed.wrapping_mul(i + 1));
        super::restore_refresh_active_env(Some(std::ffi::OsString::from(&marker)));
        assert_eq!(std::env::var(key).unwrap(), marker);
        super::restore_refresh_active_env(None);
        assert!(std::env::var_os(key).is_none());
    }
    match previous {
        Some(value) => unsafe { std::env::set_var(key, value) },
        None => unsafe { std::env::remove_var(key) },
    }
}

#[test]
fn ensure_check_runtime_coverage_on_empty_repo_reports_refresh_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    let required = crate::test_runner::check_line_coverage::RequiredCoverageLanguages {
        python: false,
        rust: true,
    };
    let err = super::ensure_check_runtime_coverage(repo, required, &[], 1).unwrap_err();
    let rendered = err.to_string();
    assert!(rendered.contains("runtime line coverage"), "{rendered}");
}

#[test]
fn try_repair_rust_check_aggregate_returns_none_or_discovery_error_on_empty_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let result =
        super::try_repair_rust_check_aggregate(tmp.path(), &[], &["pkg::bin$alpha".into()], 1);
    match result {
        Ok(None) => {}
        Err(err) => {
            let rendered = err.to_string();
            assert!(
                rendered.contains("discovery") || rendered.contains("runtime line coverage"),
                "{rendered}"
            );
        }
        Ok(Some(_)) => panic!("empty repo must not produce repair stats"),
    }
}

fn bare_crate_with_lib(tmp: &tempfile::TempDir) {
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname=\"t\"\nversion=\"0.0.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("src/lib.rs"),
        "pub fn covered() {}\n\
         #[cfg(test)]\n\
         mod tests {\n\
             #[test]\n\
             fn missing_case() {\n\
                 assert_eq!(super::covered(), ());\n\
             }\n\
         }\n",
    )
    .unwrap();
}

fn inject_synthetic_binary_into_index(
    build: &mut crate::test_runner::rust_llvm_cov::RustExecutableIndexBuild,
    selector: &str,
    binary_id: &str,
) -> rust_llvm_cov_runner::RustLineCoverage {
    let exe = build.request.source_root.join("target").join(binary_id);
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, b"synthetic-test-binary").unwrap();
    build.index.test_binaries = vec![rust_llvm_cov_runner::RustTestBinaryIdentity {
        id: binary_id.to_string(),
        executable: exe.to_string_lossy().to_string(),
        digest: "synthetic-digest".to_string(),
    }];
    build.index.selector_binary_ids =
        std::collections::BTreeMap::from([(selector.to_string(), vec![binary_id.to_string()])]);
    rust_llvm_cov_runner::RustLineCoverage {
        files: std::collections::BTreeMap::from([(
            "src/lib.rs".to_string(),
            std::collections::BTreeSet::from([1]),
        )]),
    }
}

#[test]
fn apply_identity_only_repair_publishes_when_maps_match_injected_index() {
    let tmp = tempfile::tempdir().unwrap();
    bare_crate_with_lib(&tmp);
    let selectors =
        crate::test_runner::runners::enumerate_workspace_rust_selectors(tmp.path(), &[])
            .expect("bare crate with a unit test should enumerate selectors");
    assert!(
        !selectors.is_empty(),
        "expected at least one rust selector, got {selectors:?}"
    );
    let mut build = crate::test_runner::rust_llvm_cov::build_current_rust_test_executable_index(
        tmp.path(),
        &selectors,
        &[],
        1,
    )
    .expect("bare crate can build an executable index");
    let line_map = inject_synthetic_binary_into_index(&mut build, &selectors[0], "bin-a");
    // Keep selector→binary map aligned for every discovered selector.
    for selector in &selectors {
        build
            .index
            .selector_binary_ids
            .entry(selector.clone())
            .or_insert_with(|| vec!["bin-a".to_string()]);
    }
    let retained = std::collections::BTreeMap::from([("bin-a".to_string(), line_map)]);
    let stats = super::apply_identity_only_repair(tmp.path(), &[], &build, &selectors, retained)
        .expect("identity-only repair should publish a valid aggregate");
    assert!(stats.rust_identity_only_repair);
    assert_eq!(stats.rust_aggregate_binaries, 1);
}

#[test]
fn finalize_population_summary_maps_nonzero_exit_to_test_execution() {
    let tmp = tempfile::tempdir().unwrap();
    let summary = crate::test_runner::runners::SelectorExecutionSummary {
        exit_code: 7,
        total: 4,
        failed: 2,
        rust_aggregate_binaries: 3,
        rust_aggregate_exports: 1,
        ..Default::default()
    };
    let err = super::finalize_population_summary(tmp.path(), &[], &summary, false).unwrap_err();
    match err {
        super::CoverageRefreshError::TestExecution {
            language,
            total,
            failed,
            exit_code,
        } => {
            assert_eq!(language, "Rust");
            assert_eq!(total, 4);
            assert_eq!(failed, 2);
            assert_eq!(exit_code, 7);
        }
        other => panic!("expected TestExecution, got {other:?}"),
    }
}

#[test]
fn finalize_population_summary_accepts_zero_exit_after_identity_publish() {
    let tmp = tempfile::tempdir().unwrap();
    bare_crate_with_lib(&tmp);
    let selectors =
        crate::test_runner::runners::enumerate_workspace_rust_selectors(tmp.path(), &[])
            .expect("selectors");
    let mut build = crate::test_runner::rust_llvm_cov::build_current_rust_test_executable_index(
        tmp.path(),
        &selectors,
        &[],
        1,
    )
    .expect("index");
    let line_map = inject_synthetic_binary_into_index(&mut build, &selectors[0], "bin-a");
    for selector in &selectors {
        build
            .index
            .selector_binary_ids
            .entry(selector.clone())
            .or_insert_with(|| vec!["bin-a".to_string()]);
    }
    super::apply_identity_only_repair(
        tmp.path(),
        &[],
        &build,
        &selectors,
        std::collections::BTreeMap::from([("bin-a".to_string(), line_map)]),
    )
    .expect("publish");
    let summary = crate::test_runner::runners::SelectorExecutionSummary {
        exit_code: 0,
        rust_test_instances: 2,
        rust_aggregate_binaries: 1,
        rust_aggregate_exports: 1,
        ..Default::default()
    };
    let stats = super::finalize_population_summary(tmp.path(), &[], &summary, true)
        .expect("zero-exit summary should validate published aggregate");
    assert!(stats.rust_full_refresh);
    assert_eq!(stats.rust_aggregate_binaries, 1);
    assert_eq!(stats.rust_aggregate_exports, 1);
    assert_eq!(stats.rust_test_instances, 2);
}

#[test]
fn finalize_population_summary_full_refresh_flag_is_metamorphic() {
    let tmp = tempfile::tempdir().unwrap();
    let summary = crate::test_runner::runners::SelectorExecutionSummary {
        exit_code: 1,
        total: 1,
        failed: 1,
        ..Default::default()
    };
    let a = super::finalize_population_summary(tmp.path(), &[], &summary, false).unwrap_err();
    let b = super::finalize_population_summary(tmp.path(), &[], &summary, true).unwrap_err();
    assert_eq!(a.to_string(), b.to_string());
}

#[test]
fn apply_identity_only_repair_on_bare_index_reports_structured_failure() {
    let tmp = tempfile::tempdir().unwrap();
    bare_crate_with_lib(&tmp);
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
    bare_crate_with_lib(&tmp);
    let build = crate::test_runner::rust_llvm_cov::build_current_rust_test_executable_index(
        tmp.path(),
        &["missing_case".into()],
        &[],
        1,
    )
    .expect("bare crate can build an executable index");
    let err = super::apply_rerun_repair(
        tmp.path(),
        &[],
        &build,
        vec!["missing_case".into()],
        std::collections::BTreeSet::from(["bin".into()]),
        std::collections::BTreeMap::new(),
        1,
    )
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
    bare_crate_with_lib(&tmp);
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
        std::collections::BTreeMap::new(),
    )
    .unwrap_err()
    .to_string();
    let rerun_err = super::apply_rerun_repair(
        tmp.path(),
        &[],
        &build,
        vec!["missing_case".into()],
        std::collections::BTreeSet::from(["bin".into()]),
        std::collections::BTreeMap::new(),
        1,
    )
    .unwrap_err()
    .to_string();
    assert!(
        identity_err.contains("Rust") && rerun_err.contains("Rust"),
        "identity={identity_err} rerun={rerun_err}"
    );
}
