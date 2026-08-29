use std::collections::BTreeMap;
use std::io;

use crate::rust_llvm_cov_runner::plan::batch_plan::RustCoverageBatchRequest;
use crate::rust_llvm_cov_runner::plan::shared_input::rust_input_snapshot;
use crate::rust_llvm_cov_runner::{BATCH_EXECUTION_POLICY_VERSION, CACHE_SCHEMA_VERSION};

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
    let _ = crate::rust_llvm_cov_runner::plan::batch_identity_seal::write_identity_mtime_seal(
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
    h = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(h, selector.as_bytes());
    h = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(h, &[0]);
    h = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(
        h,
        req.cache_policy.digest().as_bytes(),
    );
    h = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(h, &[0]);
    h = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(
        h,
        req.cache_policy.effective_digest(selector).as_bytes(),
    );
    h = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(h, &[0]);
    for path in req.cache_policy.declared_paths(selector) {
        h = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(h, path.as_bytes());
        h = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(h, &[0]);
        let bytes = std::fs::read(req.source_root.join(&path)).unwrap_or_default();
        h = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(h, &bytes);
        h = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(h, &[0]);
    }
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
    let mut h = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(
        0xcbf2_9ce4_8422_2325,
        b"batch-fingerprint-v1",
    );
    for part in [
        CACHE_SCHEMA_VERSION.as_bytes(),
        crate::rust_llvm_cov_runner::CACHE_POLICY_SCHEMA_VERSION.as_bytes(),
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
        h = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(h, part);
        h = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(h, &[0]);
    }
    h = hash_env(&mut h, &req.env);
    h = hash_string_list(&mut h, &req.cargo_args);
    hash_string_list(
        &mut h,
        &crate::rust_llvm_cov_runner::plan::batch_plan_test_args::identity_relevant_test_args(
            &req.test_args,
        ),
    )
}

fn hash_env(h: &mut u64, env: &BTreeMap<String, String>) -> u64 {
    let mut acc = *h;
    for (key, value) in env {
        acc = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(acc, key.as_bytes());
        acc = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(acc, b"=");
        acc = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(acc, value.as_bytes());
        acc = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(acc, &[0]);
    }
    acc
}

fn hash_string_list(h: &mut u64, values: &[String]) -> u64 {
    let mut acc = *h;
    for value in values {
        acc = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(acc, value.as_bytes());
        acc = crate::rust_llvm_cov_runner::rust_cov_cache::rust_cov_fnv1a64(acc, &[0]);
    }
    acc
}

#[cfg(test)]
#[path = "batch_fingerprint_test.rs"]
mod tests;
