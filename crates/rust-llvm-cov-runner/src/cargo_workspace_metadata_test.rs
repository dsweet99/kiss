use std::path::{Path, PathBuf};

use crate::cargo_workspace_metadata::{
    CargoMetadata, CargoMetadataDependency, CargoMetadataTarget, WorkspaceMetadata,
    workspace_package_for_test,
};

#[test]
fn default_target_and_dependency_structs_are_empty() {
    assert!(CargoMetadataTarget::default().kind.is_empty());
    assert!(CargoMetadataDependency::default().name.is_empty());
    assert_eq!(CargoMetadataDependency::default().path, None);
}

#[test]
fn workspace_package_record_fields_are_accessible_to_tests() {
    let record = crate::cargo_workspace_metadata::WorkspacePackageRecord {
        package: workspace_package_for_test("pkg", "name", PathBuf::from("/repo")),
        has_proc_macro: false,
    };
    assert!(!record.has_proc_macro);
}

#[test]
fn effective_manifest_path_supports_split_and_joined_flags() {
    let joined = crate::cargo_workspace_metadata::effective_manifest_path(
        std::path::Path::new("/repo"),
        &["--manifest-path=/joined/Cargo.toml".to_string()],
    );
    let split = crate::cargo_workspace_metadata::effective_manifest_path(
        std::path::Path::new("/repo"),
        &[
            "--manifest-path".to_string(),
            "/split/Cargo.toml".to_string(),
        ],
    );
    assert_eq!(joined, std::path::PathBuf::from("/joined/Cargo.toml"));
    assert_eq!(split, std::path::PathBuf::from("/split/Cargo.toml"));
}

#[test]
fn cargo_metadata_target_and_dependency_fields_round_trip() {
    let dep: CargoMetadataDependency =
        serde_json::from_value(serde_json::json!({"name": "dep", "path": "/repo/dep"})).unwrap();
    let target: CargoMetadataTarget =
        serde_json::from_value(serde_json::json!({"kind": ["lib"], "crate_types": ["lib"]}))
            .unwrap();
    assert_eq!(dep.name, "dep");
    assert_eq!(target.kind, ["lib"]);
    assert_eq!(target.crate_types, ["lib"]);
}

#[test]
fn temp_repo_ordinary_lib_rs_classifies_as_non_compile_time() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let metadata = crate::cargo_workspace_metadata::workspace_metadata_from_cargo(
        tmp.path(),
        std::path::Path::new("cargo"),
        &[],
    )
    .expect("metadata");
    assert_eq!(
        metadata.rs_compile_time_classification(tmp.path(), &tmp.path().join("src/lib.rs")),
        Some(false)
    );
}

#[test]
fn cargo_metadata_accessors_expose_workspace_fields() {
    let metadata = crate::cargo_workspace_metadata::cargo_metadata_witness_for_test();
    assert_eq!(metadata.workspace_root_path(), Some("/repo"));
    assert_eq!(metadata.workspace_member_ids(), &["pkg-id".to_string()]);
    assert_eq!(metadata.workspace_packages().len(), 1);
    assert_eq!(metadata.workspace_packages()[0].name, "pkg");
    let package = crate::cargo_workspace_metadata::workspace_package_for_test(
        "pkg-id",
        "pkg",
        PathBuf::from("/repo"),
    );
    assert_eq!(package.id, "pkg-id");
    assert_eq!(package.name, "pkg");
    assert_eq!(package.manifest_dir, PathBuf::from("/repo"));
}

#[test]
fn compile_time_closure_includes_proc_macro_local_dependency() {
    let metadata = CargoMetadata {
        packages: vec![
            crate::cargo_workspace_metadata::CargoMetadataPackage {
                id: "macro-pkg".to_string(),
                name: "macro_pkg".to_string(),
                manifest_path: "/repo/macro/Cargo.toml".to_string(),
                targets: vec![
                    serde_json::from_value(serde_json::json!({
                        "kind": ["lib"],
                        "crate_types": ["proc-macro"]
                    }))
                    .unwrap(),
                ],
                dependencies: vec![
                    serde_json::from_value(serde_json::json!({
                        "name": "helper",
                        "path": "/repo/helper"
                    }))
                    .unwrap(),
                ],
            },
            crate::cargo_workspace_metadata::CargoMetadataPackage {
                id: "helper-pkg".to_string(),
                name: "helper".to_string(),
                manifest_path: "/repo/helper/Cargo.toml".to_string(),
                targets: vec![],
                dependencies: vec![],
            },
        ],
        workspace_members: vec!["macro-pkg".to_string(), "helper-pkg".to_string()],
        workspace_root: Some("/repo".to_string()),
    };
    let workspace = WorkspaceMetadata::from_cargo_metadata(&metadata);
    let closure = workspace.compile_time_package_ids();
    assert!(closure.contains("macro-pkg"));
    assert!(closure.contains("helper-pkg"));
}

#[test]
fn compile_time_closure_does_not_treat_custom_build_package_lib_as_compile_time() {
    let metadata = CargoMetadata {
        packages: vec![crate::cargo_workspace_metadata::CargoMetadataPackage {
            id: "build-pkg".to_string(),
            name: "build_pkg".to_string(),
            manifest_path: "/repo/Cargo.toml".to_string(),
            targets: vec![
                serde_json::from_value(serde_json::json!({
                    "kind": ["custom-build"],
                    "crate_types": []
                }))
                .unwrap(),
            ],
            dependencies: vec![],
        }],
        workspace_members: vec!["build-pkg".to_string()],
        workspace_root: Some("/repo".to_string()),
    };
    let workspace = WorkspaceMetadata::from_cargo_metadata(&metadata);
    assert!(!workspace.compile_time_package_ids().contains("build-pkg"));
    assert_eq!(
        workspace
            .rs_compile_time_classification(&PathBuf::from("/repo"), Path::new("/repo/build.rs")),
        Some(true)
    );
    assert_eq!(
        workspace
            .rs_compile_time_classification(&PathBuf::from("/repo"), Path::new("/repo/src/lib.rs")),
        Some(false)
    );
}

#[test]
fn proc_macro_crate_rs_classifies_as_compile_time() {
    let metadata = CargoMetadata {
        packages: vec![crate::cargo_workspace_metadata::CargoMetadataPackage {
            id: "macro-pkg".to_string(),
            name: "macro_pkg".to_string(),
            manifest_path: "/repo/macro/Cargo.toml".to_string(),
            targets: vec![
                serde_json::from_value(serde_json::json!({
                    "kind": ["lib"],
                    "crate_types": ["proc-macro"]
                }))
                .unwrap(),
            ],
            dependencies: vec![],
        }],
        workspace_members: vec!["macro-pkg".to_string()],
        workspace_root: Some("/repo".to_string()),
    };
    let workspace = WorkspaceMetadata::from_cargo_metadata(&metadata);
    let root = PathBuf::from("/repo");
    assert_eq!(
        workspace.rs_compile_time_classification(&root, Path::new("/repo/macro/src/lib.rs")),
        Some(true)
    );
}

#[test]
fn current_package_id_matches_workspace_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let metadata = crate::cargo_workspace_metadata::load_cargo_metadata(
        tmp.path(),
        std::path::Path::new("cargo"),
        &[],
    )
    .expect("metadata");
    let package_id = metadata
        .current_package_id(tmp.path(), &[])
        .expect("current package");
    assert!(package_id.contains("demo"));
    assert_eq!(metadata.workspace_packages().len(), 1);
}

#[test]
fn effective_manifest_path_honors_manifest_path_flag() {
    let path = crate::cargo_workspace_metadata::effective_manifest_path(
        std::path::Path::new("/repo"),
        &["--manifest-path=/other/Cargo.toml".to_string()],
    );
    assert_eq!(path, std::path::PathBuf::from("/other/Cargo.toml"));
}

#[test]
fn non_rs_paths_and_parent_dirs_are_not_compile_time() {
    let metadata = CargoMetadata {
        packages: vec![crate::cargo_workspace_metadata::CargoMetadataPackage {
            id: "app-pkg".to_string(),
            name: "app".to_string(),
            manifest_path: "/repo/Cargo.toml".to_string(),
            targets: vec![],
            dependencies: vec![],
        }],
        workspace_members: vec!["app-pkg".to_string()],
        workspace_root: Some("/repo".to_string()),
    };
    let workspace = WorkspaceMetadata::from_cargo_metadata(&metadata);
    let root = PathBuf::from("/repo");
    assert_eq!(
        workspace.rs_compile_time_classification(&root, &root.join("Cargo.toml")),
        Some(false)
    );
    assert_eq!(
        workspace.rs_compile_time_classification(&root, Path::new("../outside.rs")),
        None
    );
}

#[test]
fn ordinary_rs_outside_compile_time_closure_classifies_as_non_compile_time() {
    let metadata = CargoMetadata {
        packages: vec![crate::cargo_workspace_metadata::CargoMetadataPackage {
            id: "app-pkg".to_string(),
            name: "app".to_string(),
            manifest_path: "/repo/Cargo.toml".to_string(),
            targets: vec![
                serde_json::from_value(serde_json::json!({
                    "kind": ["lib"],
                    "crate_types": ["lib"]
                }))
                .unwrap(),
            ],
            dependencies: vec![],
        }],
        workspace_members: vec!["app-pkg".to_string()],
        workspace_root: Some("/repo".to_string()),
    };
    let workspace = WorkspaceMetadata::from_cargo_metadata(&metadata);
    let root = PathBuf::from("/repo");
    assert_eq!(
        workspace.rs_compile_time_classification(&root, &root.join("src/lib.rs")),
        Some(false)
    );
}
