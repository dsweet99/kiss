use std::collections::BTreeMap;
use std::io;

use crate::plan::batch_plan::RustCoverageBatchRequest;
use crate::plan::shared_input::rust_input_snapshot;
use crate::{BATCH_EXECUTION_POLICY_VERSION, CACHE_SCHEMA_VERSION};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustCoverageToolIdentity {
    pub cargo_version: String,
    pub llvm_cov_version: String,
    pub rustc_version: String,
    pub cargo_nextest_version: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RustCoverageBatchIdentity {
    pub input_digest: String,
    pub generation_fingerprint: String,
    pub selection_context_fingerprint: String,
    pub ordinary_source_digests: BTreeMap<String, String>,
}

pub fn batch_identity(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
) -> io::Result<RustCoverageBatchIdentity> {
    if let Some(cached) =
        crate::plan::batch_identity_seal::try_identity_from_mtime_seal(&req.cache_root, &req.source_root, req, tools)
    {
        return Ok(cached);
    }
    let snapshot = rust_input_snapshot(&req.source_root, req)
        .map_err(|err| io::Error::other(format!("{err:?}")))?;
    let generation_fingerprint = generation_fingerprint(
        &snapshot.input_digest,
        req,
        tools,
        BATCH_EXECUTION_POLICY_VERSION,
    );
    let selection_context_fingerprint = selection_context_fingerprint(
        &snapshot.selection_context_source_digest,
        req,
        tools,
        BATCH_EXECUTION_POLICY_VERSION,
    );
    let identity = RustCoverageBatchIdentity {
        input_digest: snapshot.input_digest,
        generation_fingerprint,
        selection_context_fingerprint,
        ordinary_source_digests: snapshot.ordinary_source_digests,
    };
    let _ = crate::plan::batch_identity_seal::write_identity_mtime_seal(
        &req.cache_root,
        &req.source_root,
        req,
        tools,
        &identity,
    );
    Ok(identity)
}

pub fn entry_fingerprint(
    input_digest: &str,
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    selector: &str,
) -> String {
    let mut h = generation_hash(input_digest, req, tools, BATCH_EXECUTION_POLICY_VERSION);
    h = crate::rust_cov_cache::rust_cov_fnv1a64(h, selector.as_bytes());
    h = crate::rust_cov_cache::rust_cov_fnv1a64(h, &[0]);
    format!("{h:016x}")
}

pub(crate) fn generation_fingerprint(
    input_digest: &str,
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    execution_policy: &str,
) -> String {
    format!(
        "{:016x}",
        generation_hash(input_digest, req, tools, execution_policy)
    )
}

pub(crate) fn selection_context_fingerprint(
    selection_context_source: &str,
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    execution_policy: &str,
) -> String {
    format!(
        "{:016x}",
        generation_hash(selection_context_source, req, tools, execution_policy)
    )
}

fn generation_hash(
    source_digest: &str,
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    execution_policy: &str,
) -> u64 {
    let mut h =
        crate::rust_cov_cache::rust_cov_fnv1a64(0xcbf2_9ce4_8422_2325, b"batch-fingerprint-v1");
    for part in [
        CACHE_SCHEMA_VERSION.as_bytes(),
        source_digest.as_bytes(),
        execution_policy.as_bytes(),
        req.runner_map_fingerprint.as_bytes(),
        tools.cargo_version.as_bytes(),
        tools.llvm_cov_version.as_bytes(),
        tools.rustc_version.as_bytes(),
        tools.cargo_nextest_version.as_bytes(),
        req.cwd.to_string_lossy().as_bytes(),
        req.source_root.to_string_lossy().as_bytes(),
    ] {
        h = crate::rust_cov_cache::rust_cov_fnv1a64(h, part);
        h = crate::rust_cov_cache::rust_cov_fnv1a64(h, &[0]);
    }
    h = hash_env(&mut h, &req.env);
    h = hash_string_list(&mut h, &req.cargo_args);
    hash_string_list(&mut h, &req.test_args)
}

fn hash_env(h: &mut u64, env: &BTreeMap<String, String>) -> u64 {
    let mut acc = *h;
    for (key, value) in env {
        acc = crate::rust_cov_cache::rust_cov_fnv1a64(acc, key.as_bytes());
        acc = crate::rust_cov_cache::rust_cov_fnv1a64(acc, b"=");
        acc = crate::rust_cov_cache::rust_cov_fnv1a64(acc, value.as_bytes());
        acc = crate::rust_cov_cache::rust_cov_fnv1a64(acc, &[0]);
    }
    acc
}

fn hash_string_list(h: &mut u64, values: &[String]) -> u64 {
    let mut acc = *h;
    for value in values {
        acc = crate::rust_cov_cache::rust_cov_fnv1a64(acc, value.as_bytes());
        acc = crate::rust_cov_cache::rust_cov_fnv1a64(acc, &[0]);
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::batch_plan::RustCoverageBatchRequest;
    use crate::test_support::witness_batch_tools;
    use std::fs;

    fn tools() -> RustCoverageToolIdentity {
        witness_batch_tools()
    }

    fn request() -> RustCoverageBatchRequest {
        RustCoverageBatchRequest::witness()
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
    fn ordinary_source_edit_changes_generation_but_not_selection_context() {
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
        let _ = crate::plan::cargo_workspace_metadata::workspace_metadata_from_cargo(
            &req.cwd,
            &req.cargo,
            &req.cargo_args,
        );
        let before = batch_identity(&req, &tools()).unwrap();
        fs::write(tmp.path().join("src").join("lib.rs"), "pub fn y() {}\n").unwrap();
        let after = batch_identity(&req, &tools()).unwrap();
        assert_ne!(before.input_digest, after.input_digest);
        assert_ne!(before.generation_fingerprint, after.generation_fingerprint);
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
    fn ordinary_test_file_edit_changes_generation_but_not_selection_context() {
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
        let _ = crate::plan::cargo_workspace_metadata::workspace_metadata_from_cargo(
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
        assert_ne!(before.input_digest, after.input_digest);
        assert_ne!(before.generation_fingerprint, after.generation_fingerprint);
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
        let _ = crate::plan::cargo_workspace_metadata::workspace_metadata_from_cargo(
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
}
