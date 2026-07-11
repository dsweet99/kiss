use std::collections::BTreeMap;
use std::io;

use crate::batch_plan::RustCoverageBatchRequest;
use crate::rust_cov_cache::rust_cov_fnv1a64;
use crate::shared_input::workspace_input_digest;
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
}

pub fn batch_identity(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
) -> io::Result<RustCoverageBatchIdentity> {
    let input_digest = workspace_input_digest(&req.source_root)?;
    let generation_fingerprint =
        generation_fingerprint(&input_digest, req, tools, BATCH_EXECUTION_POLICY_VERSION);
    Ok(RustCoverageBatchIdentity {
        input_digest,
        generation_fingerprint,
    })
}

pub fn entry_fingerprint(
    input_digest: &str,
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    selector: &str,
) -> String {
    let mut h = generation_hash(input_digest, req, tools, BATCH_EXECUTION_POLICY_VERSION);
    h = rust_cov_fnv1a64(h, selector.as_bytes());
    h = rust_cov_fnv1a64(h, &[0]);
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

fn generation_hash(
    input_digest: &str,
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    execution_policy: &str,
) -> u64 {
    let mut h = rust_cov_fnv1a64(0xcbf2_9ce4_8422_2325, b"batch-fingerprint-v1");
    for part in [
        CACHE_SCHEMA_VERSION.as_bytes(),
        input_digest.as_bytes(),
        execution_policy.as_bytes(),
        req.runner_map_fingerprint.as_bytes(),
        tools.cargo_version.as_bytes(),
        tools.llvm_cov_version.as_bytes(),
        tools.rustc_version.as_bytes(),
        tools.cargo_nextest_version.as_bytes(),
        req.cwd.to_string_lossy().as_bytes(),
        req.source_root.to_string_lossy().as_bytes(),
    ] {
        h = rust_cov_fnv1a64(h, part);
        h = rust_cov_fnv1a64(h, &[0]);
    }
    h = hash_env(&mut h, &req.env);
    h = hash_string_list(&mut h, &req.cargo_args);
    hash_string_list(&mut h, &req.test_args)
}

fn hash_env(h: &mut u64, env: &BTreeMap<String, String>) -> u64 {
    let mut acc = *h;
    for (key, value) in env {
        acc = rust_cov_fnv1a64(acc, key.as_bytes());
        acc = rust_cov_fnv1a64(acc, b"=");
        acc = rust_cov_fnv1a64(acc, value.as_bytes());
        acc = rust_cov_fnv1a64(acc, &[0]);
    }
    acc
}

fn hash_string_list(h: &mut u64, values: &[String]) -> u64 {
    let mut acc = *h;
    for value in values {
        acc = rust_cov_fnv1a64(acc, value.as_bytes());
        acc = rust_cov_fnv1a64(acc, &[0]);
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch_plan::RustCoverageBatchRequest;
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
        };
        assert_eq!(tools.rustc_version, "rustc");
        assert_eq!(identity.input_digest, "abc");
    }
}
