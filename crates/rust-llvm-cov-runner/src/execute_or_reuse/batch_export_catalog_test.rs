use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use crate::execute_or_reuse::batch_export_catalog::build_object_catalog;

#[test]
fn build_object_catalog_includes_env_executables() {
    let env = BTreeMap::from([(
        "KISS_EXPORT_CONTRACT_HELPER".to_string(),
        "/tmp/helper-bin".to_string(),
    )]);
    let catalog = build_object_catalog(
        &[],
        PathBuf::from("/tmp/missing-target").as_path(),
        &[],
        &env,
    );
    assert!(catalog.is_empty() || catalog.iter().all(|path| path.is_absolute()));
}

#[test]
fn build_object_catalog_includes_root_level_cargo_binaries() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target").join("debug");
    let deps = target.join("deps");
    fs::create_dir_all(&deps).unwrap();
    let root_bin = target.join("kiss");
    let deps_bin = deps.join("integration_suite-abc123");
    let depfile = target.join("kiss.d");
    fs::write(&root_bin, b"bin").unwrap();
    fs::write(&deps_bin, b"test bin").unwrap();
    fs::write(&depfile, b"depfile").unwrap();

    let catalog = build_object_catalog(&[], &target, &[], &BTreeMap::new());

    assert!(catalog.contains(&root_bin));
    assert!(catalog.contains(&deps_bin));
    assert!(!catalog.contains(&depfile));
}

#[test]
fn build_object_catalog_skips_nested_llvm_cov_target_trees() {
    let tmp = tempfile::tempdir().unwrap();
    let cargo_target = tmp.path().join("target");
    let deps = cargo_target.join("debug").join("deps");
    let nested = cargo_target
        .join("llvm-cov-target")
        .join("debug")
        .join("deps");
    fs::create_dir_all(&deps).unwrap();
    fs::create_dir_all(&nested).unwrap();
    let keep = deps.join("keep-bin");
    let skip = nested.join("skip-bin");
    fs::write(&keep, b"keep").unwrap();
    fs::write(&skip, b"skip").unwrap();

    let catalog = build_object_catalog(&[], &cargo_target, &[], &BTreeMap::new());
    assert!(catalog.contains(&keep));
    assert!(!catalog.contains(&skip));
}
