use super::*;
use crate::rust_llvm_cov_runner::plan::batch_plan::RustCoverageBatchRequest;
use crate::rust_llvm_cov_runner::test_support::witness_batch_tools;
use std::fs;

fn tools() -> RustCoverageToolIdentity {
    witness_batch_tools()
}

fn request() -> RustCoverageBatchRequest {
    RustCoverageBatchRequest::witness()
}

fn write_executable_stub(path: &std::path::Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"stub\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn entry_fingerprints_differ_by_selector_but_share_generation() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("Cargo.toml"), "[package]\n").unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();

    let mut req = request();
    req.source_root = tmp.path().to_path_buf();
    req.cwd = tmp.path().to_path_buf();
    let identity = batch_identity(&req, &tools()).unwrap();
    let alpha = entry_fingerprint(&identity.input_digest, &req, &tools(), "alpha");
    let beta = entry_fingerprint(&identity.input_digest, &req, &tools(), "beta");
    assert_ne!(alpha, beta);
    assert_eq!(
        identity.generation_fingerprint,
        generation_fingerprint(
            &identity.input_digest,
            &req,
            &tools(),
            BATCH_EXECUTION_POLICY_VERSION
        )
    );
}

#[test]
fn generation_fingerprint_excludes_selector() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("Cargo.toml"), "[package]\n").unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();

    let mut req_a = request();
    req_a.source_root = tmp.path().to_path_buf();
    req_a.cwd = tmp.path().to_path_buf();
    let mut req_b = req_a.clone();
    req_b.logical_selectors = vec!["other".to_string()];
    let id_a = batch_identity(&req_a, &tools()).unwrap();
    let id_b = batch_identity(&req_b, &tools()).unwrap();
    assert_eq!(id_a.generation_fingerprint, id_b.generation_fingerprint);
}

#[test]
fn generation_fingerprint_tracks_env_and_tool_identity_fields() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("Cargo.toml"), "[package]\n").unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();

    let mut req = request();
    req.source_root = tmp.path().to_path_buf();
    req.cwd = tmp.path().to_path_buf();
    req.env.insert("K".to_string(), "V".to_string());
    let with_env = batch_identity(&req, &tools()).unwrap();
    req.env.clear();
    let without_env = batch_identity(&req, &tools()).unwrap();
    assert_ne!(
        with_env.generation_fingerprint,
        without_env.generation_fingerprint
    );

    let tools_a = tools();
    let mut tools_b = tools();
    tools_b.cargo_nextest_version = "cargo-nextest 0.10".to_string();
    let id_a = batch_identity(&req, &tools_a).unwrap();
    let id_b = batch_identity(&req, &tools_b).unwrap();
    assert_ne!(id_a.generation_fingerprint, id_b.generation_fingerprint);
    assert_eq!(tools_a.cargo_nextest_version, "cargo-nextest 0.9");
}

#[test]
fn generation_fingerprint_tracks_path_changes() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("Cargo.toml"), "[package]\n").unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let old_bin = tmp.path().join("old-bin");
    let new_bin = tmp.path().join("new-bin");
    write_executable_stub(&old_bin.join("cmake"));
    write_executable_stub(&new_bin.join("cmake"));

    let mut req = request();
    req.source_root = tmp.path().to_path_buf();
    req.cwd = tmp.path().to_path_buf();
    req.env
        .insert("PATH".to_string(), old_bin.to_string_lossy().into_owned());
    let old = batch_identity(&req, &tools()).unwrap();
    req.env
        .insert("PATH".to_string(), new_bin.to_string_lossy().into_owned());
    let new = batch_identity(&req, &tools()).unwrap();

    assert_ne!(old.generation_fingerprint, new.generation_fingerprint);
}

#[test]
fn generation_fingerprint_ignores_unused_path_prefix() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("Cargo.toml"), "[package]\n").unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let tool_bin = tmp.path().join("tool-bin");
    let unused_bin = tmp.path().join("unused-bin");
    write_executable_stub(&tool_bin.join("cmake"));
    fs::create_dir_all(&unused_bin).unwrap();
    let separator = if cfg!(windows) { ';' } else { ':' };

    let mut req = request();
    req.source_root = tmp.path().to_path_buf();
    req.cwd = tmp.path().to_path_buf();
    req.env
        .insert("PATH".to_string(), tool_bin.to_string_lossy().into_owned());
    let original = batch_identity(&req, &tools()).unwrap();
    req.env.insert(
        "PATH".to_string(),
        format!("{}{separator}{}", unused_bin.display(), tool_bin.display()),
    );
    let prefixed = batch_identity(&req, &tools()).unwrap();

    assert_eq!(
        original.generation_fingerprint,
        prefixed.generation_fingerprint
    );
}

#[test]
fn generation_fingerprint_normalizes_effective_child_environment() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("Cargo.toml"), "[package]\n").unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    let separator = if cfg!(windows) { ';' } else { ':' };

    let mut req = request();
    req.source_root = tmp.path().to_path_buf();
    req.cwd = tmp.path().to_path_buf();
    req.env.insert(
        "PATH".to_string(),
        ["/one", "/two"].join(&separator.to_string()),
    );
    req.env.insert(
        "LLVM_PROFILE_FILE".to_string(),
        "/outer/old.profraw".to_string(),
    );
    let old = batch_identity(&req, &tools()).unwrap();
    req.env.insert(
        "PATH".to_string(),
        ["/one", "/two", "/one"].join(&separator.to_string()),
    );
    req.env.insert(
        "LLVM_PROFILE_FILE".to_string(),
        "/outer/new.profraw".to_string(),
    );
    let new = batch_identity(&req, &tools()).unwrap();

    assert_eq!(old.generation_fingerprint, new.generation_fingerprint);
    assert_eq!(
        old.selection_context_fingerprint,
        new.selection_context_fingerprint
    );
}

#[test]
fn generation_fingerprint_ignores_inherited_plan_owned_environment() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("Cargo.toml"), "[package]\n").unwrap();
    fs::write(tmp.path().join("src/lib.rs"), "pub fn x() {}\n").unwrap();

    let mut req = request();
    req.source_root = tmp.path().to_path_buf();
    req.cwd = tmp.path().to_path_buf();
    req.cache_root = tmp.path().join(".kiss/rust_llvm_cov_cache");
    let base = batch_identity(&req, &tools()).unwrap();
    for key in [
        "CARGO_TARGET_DIR",
        "CARGO_LLVM_COV_TARGET_DIR",
        "CARGO_LLVM_COV_BUILD_DIR",
        "CARGO_INCREMENTAL",
        "NEXTEST_EXPERIMENTAL_LIBTEST_JSON",
        "KISS_RUST_COVERAGE_PROFILE_POOL",
    ] {
        req.env.insert(key.to_string(), "inherited".to_string());
    }

    let with_inherited = batch_identity(&req, &tools()).unwrap();

    assert_eq!(
        with_inherited.generation_fingerprint,
        base.generation_fingerprint
    );
    assert_eq!(
        with_inherited.selection_context_fingerprint,
        base.selection_context_fingerprint
    );
}

#[test]
fn generation_fingerprint_ignores_aggregate_profile_file_name() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(tmp.path().join("Cargo.toml"), "[package]\n").unwrap();
    fs::write(tmp.path().join("src/lib.rs"), "pub fn x() {}\n").unwrap();

    let mut req = request();
    req.source_root = tmp.path().to_path_buf();
    req.cwd = tmp.path().to_path_buf();
    let selector_entries = batch_identity(&req, &tools()).unwrap();
    req.coverage_output_mode =
        crate::rust_llvm_cov_runner::plan::batch_plan::CoverageOutputMode::CheckAggregate {
            publication_binary_ids: None,
            repair_publication: None,
        };
    let check_aggregate = batch_identity(&req, &tools()).unwrap();

    assert_eq!(
        selector_entries.generation_fingerprint,
        check_aggregate.generation_fingerprint
    );
}

#[test]
fn ordinary_source_edit_does_not_change_generation() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();

    let mut req = request();
    req.source_root = tmp.path().to_path_buf();
    req.cwd = tmp.path().to_path_buf();
    req.cargo_args.clear();
    let _ =
        crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::workspace_metadata_from_cargo(
            &req.cwd,
            &req.cargo,
            &req.cargo_args,
        );
    let before = batch_identity(&req, &tools()).unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "pub fn y() {}\n").unwrap();
    let after = batch_identity(&req, &tools()).unwrap();
    assert_eq!(before.input_digest, after.input_digest);
    assert_eq!(before.generation_fingerprint, after.generation_fingerprint);
    assert_eq!(before.ordinary_source_digests.len(), 1);
    assert_eq!(after.ordinary_source_digests.len(), 1);
    assert_ne!(
        before.ordinary_source_digests.get("src/lib.rs"),
        after.ordinary_source_digests.get("src/lib.rs")
    );
    assert_eq!(
        before.selection_context_fingerprint,
        after.selection_context_fingerprint
    );
}

#[test]
fn ordinary_test_file_edit_does_not_change_generation() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::create_dir_all(tmp.path().join("tests")).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();
    fs::write(
        tmp.path().join("tests").join("integration.rs"),
        "#[test] fn passes() { assert!(true); }\n",
    )
    .unwrap();

    let mut req = request();
    req.source_root = tmp.path().to_path_buf();
    req.cwd = tmp.path().to_path_buf();
    req.cargo_args.clear();
    let _ =
        crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::workspace_metadata_from_cargo(
            &req.cwd,
            &req.cargo,
            &req.cargo_args,
        );
    let before = batch_identity(&req, &tools()).unwrap();
    fs::write(
        tmp.path().join("tests").join("integration.rs"),
        "#[test] fn passes() { assert!(true); assert!(true); }\n",
    )
    .unwrap();
    let after = batch_identity(&req, &tools()).unwrap();
    assert_eq!(before.input_digest, after.input_digest);
    assert_eq!(before.generation_fingerprint, after.generation_fingerprint);
    assert_eq!(
        before.selection_context_fingerprint,
        after.selection_context_fingerprint
    );
}

#[test]
fn cargo_manifest_edit_changes_selection_context_fingerprint() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();

    let mut req = request();
    req.source_root = tmp.path().to_path_buf();
    req.cwd = tmp.path().to_path_buf();
    req.cargo_args.clear();
    let before = batch_identity(&req, &tools()).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.1'\nedition='2024'\n",
    )
    .unwrap();
    let after = batch_identity(&req, &tools()).unwrap();
    assert_ne!(
        before.selection_context_fingerprint,
        after.selection_context_fingerprint
    );
}

#[test]
fn allowlisted_env_change_changes_selection_context_fingerprint() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\n",
    )
    .unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();

    let mut req = request();
    req.source_root = tmp.path().to_path_buf();
    req.cwd = tmp.path().to_path_buf();
    req.env
        .insert("BUILD_SCRIPT_INPUT".to_string(), "alpha".to_string());
    let before = batch_identity(&req, &tools()).unwrap();
    req.env
        .insert("BUILD_SCRIPT_INPUT".to_string(), "beta".to_string());
    let after = batch_identity(&req, &tools()).unwrap();
    assert_ne!(
        before.selection_context_fingerprint,
        after.selection_context_fingerprint
    );
}

#[test]
fn build_script_rs_edit_changes_selection_context_fingerprint() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join("src")).unwrap();
    fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\nedition='2024'\nbuild='build.rs'\n",
    )
    .unwrap();
    fs::write(
        tmp.path().join("build.rs"),
        "fn main() { println!(\"cargo:rerun-if-env-changed=BUILD_SCRIPT_INPUT\"); }\n",
    )
    .unwrap();
    fs::write(tmp.path().join("src").join("lib.rs"), "pub fn x() {}\n").unwrap();

    let mut req = request();
    req.source_root = tmp.path().to_path_buf();
    req.cwd = tmp.path().to_path_buf();
    req.cargo_args.clear();
    let _ =
        crate::rust_llvm_cov_runner::plan::cargo_workspace_metadata::workspace_metadata_from_cargo(
            &req.cwd,
            &req.cargo,
            &req.cargo_args,
        );
    let before = batch_identity(&req, &tools()).unwrap();
    fs::write(
        tmp.path().join("build.rs"),
        "fn main() { println!(\"cargo:rerun-if-changed=build.rs\"); }\n",
    )
    .unwrap();
    let after = batch_identity(&req, &tools()).unwrap();
    assert_ne!(
        before.selection_context_fingerprint,
        after.selection_context_fingerprint
    );
    assert_ne!(before.input_digest, after.input_digest);
}

#[test]
fn identity_structs_expose_all_tool_and_batch_fields() {
    let tools = RustCoverageToolIdentity {
        cargo_version: "cargo".to_string(),
        llvm_cov_version: "llvm-cov".to_string(),
        rustc_version: "rustc".to_string(),
        cargo_nextest_version: "nextest".to_string(),
    };
    let identity = RustCoverageBatchIdentity {
        input_digest: "abc".to_string(),
        generation_fingerprint: "def".to_string(),
        selection_context_fingerprint: "ghi".to_string(),
        ordinary_source_digests: BTreeMap::new(),
    };
    assert_eq!(tools.rustc_version, "rustc");
    assert_eq!(identity.input_digest, "abc");
    assert_eq!(identity.selection_context_fingerprint, "ghi");
}
