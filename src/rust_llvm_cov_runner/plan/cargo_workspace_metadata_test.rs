use std::path::{Path, PathBuf};

use crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::{
    CargoMetadata, CargoMetadataDependency, CargoMetadataPackage, CargoMetadataTarget,
    WorkspaceMetadata, WorkspacePackageRecord, workspace_package_for_test,
};

#[test]
fn witness_workspace_metadata_struct_fields() {
    let _ = std::mem::size_of::<WorkspacePackageRecord>();
    let _ = std::mem::size_of::<CargoMetadataTarget>();
    let _ = std::mem::size_of::<CargoMetadataDependency>();
    let package = workspace_package_for_test("pkg", "name", PathBuf::from("/repo"));
    assert_eq!(package.name, "name");
    let metadata = CargoMetadata {
        packages: vec![CargoMetadataPackage {
            id: "pkg".to_string(),
            name: "name".to_string(),
            manifest_path: "/repo/Cargo.toml".to_string(),
            targets: vec![
                serde_json::from_value(serde_json::json!({
                    "kind": ["lib"],
                    "crate_types": ["lib"]
                }))
                .unwrap(),
            ],
            dependencies: vec![
                serde_json::from_value(serde_json::json!({
                    "name": "dep",
                    "path": "/repo/dep"
                }))
                .unwrap(),
            ],
        }],
        workspace_members: vec!["pkg".to_string()],
        workspace_root: Some("/repo".to_string()),
    };
    let workspace = WorkspaceMetadata::from_cargo_metadata(&metadata);
    assert_eq!(workspace.compile_time_package_ids().len(), 0);
    let packages = metadata.workspace_packages();
    assert_eq!(packages[0].id, "pkg");
    assert_eq!(package.manifest_dir, PathBuf::from("/repo"));
}

#[test]
fn workspace_packages_excludes_registry_dependencies() {
    let package = |id: &str, name: &str, manifest_path: &str| CargoMetadataPackage {
        id: id.to_string(),
        name: name.to_string(),
        manifest_path: manifest_path.to_string(),
        targets: Vec::new(),
        dependencies: Vec::new(),
    };
    let metadata = CargoMetadata {
        packages: vec![
            package("workspace", "workspace", "/repo/Cargo.toml"),
            package(
                "registry-dependency",
                "dependency",
                "/cargo/registry/dependency/Cargo.toml",
            ),
        ],
        workspace_members: vec!["workspace".to_string()],
        workspace_root: Some("/repo".to_string()),
    };

    let packages = metadata.workspace_packages();
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].id, "workspace");
}

#[test]
fn default_target_and_dependency_structs_are_empty() {
    assert!(CargoMetadataTarget::default().kind.is_empty());
    assert!(CargoMetadataDependency::default().name.is_empty());
    assert_eq!(CargoMetadataDependency::default().path, None);
}

#[test]
fn workspace_package_record_fields_are_accessible_to_tests() {
    let record =
        crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::WorkspacePackageRecord {
            package: workspace_package_for_test("pkg", "name", PathBuf::from("/repo")),
            has_proc_macro: false,
        };
    assert!(!record.has_proc_macro);
}

#[test]
fn effective_manifest_path_supports_split_and_joined_flags() {
    let joined =
        crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::effective_manifest_path(
            std::path::Path::new("/repo"),
            &["--manifest-path=/joined/Cargo.toml".to_string()],
        );
    let split =
        crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::effective_manifest_path(
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
    let metadata =
        crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::workspace_metadata_from_cargo(
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
fn nested_non_member_crate_sources_are_detected() {
    let tmp = write_nested_non_member_fixture();
    let metadata =
        crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::workspace_metadata_from_cargo(
            tmp.path(),
            std::path::Path::new("cargo"),
            &[],
        )
        .expect("metadata");
    let nested_lib = tmp.path().join("nested/src/lib.rs");
    let member_lib = tmp.path().join("member/src/lib.rs");
    assert!(metadata.is_non_member_local_crate_source(tmp.path(), &nested_lib));
    assert!(!metadata.is_non_member_local_crate_source(tmp.path(), &member_lib));
    assert_eq!(
        metadata
            .non_member_local_crate_root(tmp.path(), &nested_lib)
            .as_deref(),
        Some("nested")
    );
}

fn write_nested_non_member_fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("member/src")).unwrap();
    std::fs::create_dir_all(tmp.path().join("nested/src")).unwrap();
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"member\"]\nresolver = \"2\"\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("member/Cargo.toml"),
        "[package]\nname='member'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("member/src/lib.rs"), "pub fn x() {}\n").unwrap();
    std::fs::write(
        tmp.path().join("nested/Cargo.toml"),
        "[package]\nname='nested'\nversion='0.1.0'\nedition='2024'\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("nested/src/lib.rs"), "pub fn y() {}\n").unwrap();
    tmp
}

#[test]
fn cargo_metadata_accessors_expose_workspace_fields() {
    let metadata = crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::cargo_metadata_witness_for_test();
    assert_eq!(metadata.workspace_root_path(), Some("/repo"));
    assert_eq!(metadata.workspace_member_ids(), &["pkg-id".to_string()]);
    assert_eq!(metadata.workspace_packages().len(), 1);
    assert_eq!(metadata.workspace_packages()[0].name, "pkg");
    let package =
        crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::workspace_package_for_test(
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
            crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::CargoMetadataPackage {
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
            crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::CargoMetadataPackage {
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
        packages: vec![
            crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::CargoMetadataPackage {
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
            },
        ],
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
        packages: vec![
            crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::CargoMetadataPackage {
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
            },
        ],
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
    let metadata =
        crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::load_cargo_metadata(
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
    let path = crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::effective_manifest_path(
        std::path::Path::new("/repo"),
        &["--manifest-path=/other/Cargo.toml".to_string()],
    );
    assert_eq!(path, std::path::PathBuf::from("/other/Cargo.toml"));
}

#[test]
fn non_rs_paths_and_parent_dirs_are_not_compile_time() {
    let metadata = CargoMetadata {
        packages: vec![
            crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::CargoMetadataPackage {
                id: "app-pkg".to_string(),
                name: "app".to_string(),
                manifest_path: "/repo/Cargo.toml".to_string(),
                targets: vec![],
                dependencies: vec![],
            },
        ],
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
        packages: vec![
            crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::CargoMetadataPackage {
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
            },
        ],
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
