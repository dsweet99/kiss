use std::path::{Path, PathBuf};

use super::{
    build_ignore_filename_regex, package_name_matches, parse_cargo_scope_args,
    resolve_ignore_filename_regex, resolve_included_packages_for_test, workspace_package_for_test,
};

#[test]
fn resolve_included_packages_honors_package_filter() {
    let packages = vec![
        workspace_package_for_test(
            "id-runner",
            "export-contract-runner",
            PathBuf::from("/fixture/runner"),
        ),
        workspace_package_for_test(
            "id-helper",
            "export-contract-helper",
            PathBuf::from("/fixture/helper"),
        ),
    ];
    let included = resolve_included_packages_for_test(
        &packages,
        &["id-runner".to_string(), "id-helper".to_string()],
        None,
        false,
        &["export-contract-runner".to_string()],
    )
    .expect("included");
    assert_eq!(included, vec!["export-contract-runner".to_string()]);
}

#[test]
fn resolve_included_packages_uses_current_package_when_present() {
    let packages = vec![
        workspace_package_for_test("id-kiss", "kiss-ai", PathBuf::from("/repo")),
        workspace_package_for_test(
            "id-runner",
            "rust-llvm-cov-runner",
            PathBuf::from("/repo/crates/runner"),
        ),
    ];
    let included = resolve_included_packages_for_test(
        &packages,
        &["id-kiss".to_string(), "id-runner".to_string()],
        Some("id-kiss"),
        false,
        &[],
    )
    .expect("included");
    assert_eq!(included, vec!["kiss-ai".to_string()]);
}

#[test]
fn resolve_included_packages_workspace_flag_includes_all_members() {
    let packages = vec![
        workspace_package_for_test("id-a", "pkg-a", PathBuf::from("/repo/a")),
        workspace_package_for_test("id-b", "pkg-b", PathBuf::from("/repo/b")),
    ];
    let included = resolve_included_packages_for_test(
        &packages,
        &["id-a".to_string(), "id-b".to_string()],
        None,
        true,
        &[],
    )
    .expect("included");
    assert_eq!(included, vec!["pkg-a".to_string(), "pkg-b".to_string()]);
}

#[test]
fn resolve_included_packages_rejects_unknown_package_filter() {
    let packages = vec![workspace_package_for_test(
        "id-runner",
        "export-contract-runner",
        PathBuf::from("/fixture/runner"),
    )];
    let err = resolve_included_packages_for_test(
        &packages,
        &["id-runner".to_string()],
        None,
        false,
        &["missing-package".to_string()],
    )
    .expect_err("expected unknown package filter error");
    assert!(matches!(
        err,
        crate::RustLlvmCovError::InvalidRequest(message) if message.contains("missing-package")
    ));
}

#[test]
fn resolve_ignore_filename_regex_fails_for_missing_manifest() {
    let req = crate::batch_plan::RustCoverageBatchRequest {
        cwd: PathBuf::from("/definitely/not/a/workspace"),
        source_root: PathBuf::from("/definitely/not/a/workspace"),
        cargo: PathBuf::from("cargo"),
        cache_root: PathBuf::from("/tmp/kiss-ignore-regex-missing"),
        logical_selectors: vec!["alpha".to_string()],
        cargo_args: vec![
            "--manifest-path".to_string(),
            "/no/such/Cargo.toml".to_string(),
        ],
        test_args: Vec::new(),
        env: Default::default(),
        force_rerun: true,
        jobs: 1,
        generated_config: PathBuf::from("/tmp/kiss-ignore-regex-missing/nextest.toml"),
        population_publication_selectors: None,
        delegated_runners: Default::default(),
        runner_map_fingerprint: String::new(),
        host_platform: String::new(),
    };
    assert!(resolve_ignore_filename_regex(&req, &req.cache_root.join("build/target")).is_err());
}

#[test]
fn resolve_ignore_filename_regex_for_kiss_repo_root() {
    let req = crate::batch_plan::RustCoverageBatchRequest {
        cwd: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("workspace root")
            .to_path_buf(),
        source_root: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("workspace root")
            .to_path_buf(),
        cargo: PathBuf::from("cargo"),
        cache_root: PathBuf::from("/tmp/kiss-ignore-regex-kiss"),
        logical_selectors: vec!["smoke".to_string()],
        cargo_args: Vec::new(),
        test_args: Vec::new(),
        env: Default::default(),
        force_rerun: true,
        jobs: 2,
        generated_config: PathBuf::from("/tmp/kiss-ignore-regex-kiss/nextest.toml"),
        population_publication_selectors: None,
        delegated_runners: Default::default(),
        runner_map_fingerprint: String::new(),
        host_platform: String::new(),
    };
    let regex = resolve_ignore_filename_regex(&req, &req.cache_root.join("build/target"))
        .expect("kiss ignore regex")
        .expect("non-empty regex");
    assert!(regex.contains("rpytest-runner") || regex.contains("crates"));
}

#[test]
fn cargo_metadata_witness_round_trips_workspace_packages() {
    let metadata = super::cargo_metadata_witness_for_test();
    let packages = metadata.workspace_packages();
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].name, "pkg");
    assert_eq!(
        metadata
            .current_package_id(Path::new("/repo"), &[])
            .as_deref(),
        Some("pkg-id")
    );
}

#[test]
fn parse_cargo_scope_args_reads_exclude_from_report() {
    let args = vec![
        "--exclude-from-report".to_string(),
        "ignored-pkg".to_string(),
    ];
    let (_, _, excluded) = parse_cargo_scope_args(&args);
    assert_eq!(excluded, vec!["ignored-pkg".to_string()]);
}

#[test]
fn resolve_included_packages_errors_when_current_package_id_unknown() {
    let packages = vec![workspace_package_for_test(
        "id-runner",
        "export-contract-runner",
        PathBuf::from("/fixture/runner"),
    )];
    let err = resolve_included_packages_for_test(
        &packages,
        &["id-runner".to_string()],
        Some("missing-id"),
        false,
        &[],
    )
    .expect_err("unknown current package");
    assert!(matches!(err, crate::RustLlvmCovError::InvalidRequest(_)));
}

#[test]
fn load_cargo_metadata_for_fixture_workspace() {
    let cwd = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/export_contract"
    ));
    let metadata = super::load_cargo_metadata_for_test(
        &cwd,
        Path::new("cargo"),
        &[
            "--manifest-path".to_string(),
            cwd.join("Cargo.toml").to_string_lossy().to_string(),
        ],
    )
    .expect("fixture metadata");
    let packages = metadata.workspace_packages();
    assert_eq!(packages.len(), 2);
    assert!(
        packages
            .iter()
            .any(|pkg| pkg.name == "export-contract-runner")
    );
    assert!(metadata.current_package_id(&cwd, &[]).is_none());
}

#[test]
fn cargo_metadata_witness_exercises_package_and_resolve_fields() {
    let metadata = super::cargo_metadata_witness_for_test();
    assert_eq!(metadata.packages()[0].id, "pkg-id");
    assert_eq!(metadata.packages()[0].manifest_path, "/repo/Cargo.toml");
    assert_eq!(metadata.workspace_root_path(), Some("/repo"));
    assert_eq!(metadata.workspace_member_ids(), &["pkg-id".to_string()]);
}

#[test]
fn ignore_filename_regex_for_workspace_packages_excludes_non_included_members() {
    let packages = vec![
        workspace_package_for_test(
            "id-runner",
            "export-contract-runner",
            PathBuf::from("/fixture/runner"),
        ),
        workspace_package_for_test(
            "id-helper",
            "export-contract-helper",
            PathBuf::from("/fixture/helper"),
        ),
    ];
    let regex = super::ignore_filename_regex_for_workspace_packages(
        Path::new("/fixture"),
        Path::new("/fixture/target"),
        &packages,
        &["id-runner".to_string(), "id-helper".to_string()],
        None,
        false,
        &["export-contract-runner".to_string()],
    )
    .expect("regex")
    .expect("non-empty");
    assert!(regex.contains("/fixture/helper"));
    assert!(packages.iter().all(|pkg| !pkg.id.is_empty()));
}

#[test]
fn ignore_filename_regex_for_empty_packages_returns_none() {
    let regex = super::ignore_filename_regex_for_workspace_packages(
        Path::new("/repo"),
        Path::new("/repo/target"),
        &[],
        &[],
        None,
        false,
        &[],
    )
    .expect("empty packages");
    assert!(regex.is_none());
}

#[test]
fn workspace_package_for_test_populates_all_fields() {
    let pkg = workspace_package_for_test("id", "name", PathBuf::from("/dir"));
    assert_eq!(pkg.id, "id");
    assert_eq!(pkg.name, "name");
    assert_eq!(pkg.manifest_dir, PathBuf::from("/dir"));
}

#[test]
fn effective_manifest_path_reads_equals_form() {
    let cwd = PathBuf::from("/repo");
    let args = vec!["--manifest-path=/repo/Cargo.toml".to_string()];
    assert_eq!(
        super::effective_manifest_path_for_test(&cwd, &args),
        PathBuf::from("/repo/Cargo.toml")
    );
}

#[test]
fn build_ignore_filename_regex_escapes_special_characters_in_paths() {
    let workspace = PathBuf::from("/tmp/plus+dir");
    let target = PathBuf::from("/tmp/plus+dir/target");
    let regex = build_ignore_filename_regex(&workspace, &target, &[]);
    assert!(regex.contains("\\+"));
}

#[test]
fn resolve_included_packages_defaults_to_all_workspace_members_without_filters() {
    let packages = vec![
        workspace_package_for_test("id-a", "pkg-a", PathBuf::from("/repo/a")),
        workspace_package_for_test("id-b", "pkg-b", PathBuf::from("/repo/b")),
    ];
    let included = resolve_included_packages_for_test(
        &packages,
        &["id-a".to_string(), "id-b".to_string()],
        None,
        false,
        &[],
    )
    .expect("all members");
    assert_eq!(included, vec!["pkg-a".to_string(), "pkg-b".to_string()]);
}

#[test]
fn parse_cargo_scope_args_reads_workspace_and_package_filters() {
    let args = vec![
        "-p".to_string(),
        "export-contract-runner".to_string(),
        "--workspace".to_string(),
        "--package=other".to_string(),
    ];
    let (workspace, packages, excluded) = parse_cargo_scope_args(&args);
    assert!(workspace);
    assert_eq!(
        packages,
        vec!["export-contract-runner".to_string(), "other".to_string()]
    );
    assert!(excluded.is_empty());
}

#[test]
fn build_ignore_filename_regex_excludes_helper_and_tests_dirs() {
    let workspace = PathBuf::from("/fixture");
    let target = PathBuf::from("/fixture/build/target");
    let regex =
        build_ignore_filename_regex(&workspace, &target, &[PathBuf::from("/fixture/helper")]);
    assert!(regex.contains("/fixture/helper($|/)"));
    assert!(regex.contains("/(tests|examples|benches)/"));
}

#[test]
fn package_name_matches_accepts_underscore_and_hyphen_aliases() {
    assert!(package_name_matches(
        "export_contract_runner",
        "export-contract-runner"
    ));
}

#[test]
fn resolve_ignore_filename_regex_for_fixture_package_filter() {
    let req = crate::batch_plan::RustCoverageBatchRequest {
        cwd: PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/export_contract"
        )),
        source_root: PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/export_contract"
        )),
        cargo: PathBuf::from("cargo"),
        cache_root: PathBuf::from("/tmp/kiss-ignore-regex"),
        logical_selectors: vec!["invokes_helper_in_process".to_string()],
        cargo_args: vec![
            "-p".to_string(),
            "export-contract-runner".to_string(),
            "--manifest-path".to_string(),
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/export_contract/Cargo.toml"
            )
            .to_string(),
        ],
        test_args: Vec::new(),
        env: Default::default(),
        force_rerun: true,
        jobs: 2,
        generated_config: PathBuf::from("/tmp/kiss-ignore-regex/nextest.toml"),
        population_publication_selectors: None,
        delegated_runners: Default::default(),
        runner_map_fingerprint: String::new(),
        host_platform: String::new(),
    };
    let regex = super::resolve_ignore_filename_regex(&req, &req.cache_root.join("build/target"))
        .expect("ignore regex");
    let regex = regex.expect("non-empty regex");
    assert!(regex.contains("helper"));
    assert!(regex.contains("tests|examples|benches"));
}
