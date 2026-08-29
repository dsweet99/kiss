use std::collections::BTreeMap;

use super::batch_fingerprint::{RustCoverageBatchIdentity, RustCoverageToolIdentity};
use super::batch_plan::RustCoverageBatchRequest;
use crate::rust_llvm_cov_runner::{
    BATCH_EXECUTION_POLICY_VERSION, CACHE_POLICY_SCHEMA_VERSION, CACHE_SCHEMA_VERSION,
};

pub const EXECUTION_CONTEXT_SCHEMA_VERSION: &str = "kiss-execution-context-v1";
pub const SOURCE_SNAPSHOT_SCHEMA_VERSION: &str = "kiss-source-snapshot-v1";
pub const TIMING_CONTEXT_SCHEMA_VERSION: &str = "kiss-timing-context-v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionContextIdentity {
    pub schema_version: String,
    pub execution_policy_version: String,
    pub cache_policy_schema_and_parser_version: String,
    pub language: String,
    pub source_root: String,
    pub canonical_tool_paths_and_content_digests: BTreeMap<String, String>,
    pub runner_map_fingerprint: String,
    pub normalized_child_environment: BTreeMap<String, String>,
    pub test_args: Vec<String>,
    pub collection_policy_or_compile_context_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSnapshot {
    pub schema_version: String,
    pub path_digests: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimingContextIdentity {
    pub schema_version: String,
    pub host_platform: String,
    pub runner_identity: String,
    pub concurrency_and_jobs: String,
    pub timeout_measurement_policy: String,
}

impl RustCoverageBatchIdentity {
    pub fn execution_context(
        &self,
        req: &RustCoverageBatchRequest,
        tools: &RustCoverageToolIdentity,
    ) -> ExecutionContextIdentity {
        ExecutionContextIdentity {
            schema_version: EXECUTION_CONTEXT_SCHEMA_VERSION.to_string(),
            execution_policy_version: BATCH_EXECUTION_POLICY_VERSION.to_string(),
            cache_policy_schema_and_parser_version: CACHE_POLICY_SCHEMA_VERSION.to_string(),
            language: "rust".to_string(),
            source_root: req.source_root.to_string_lossy().into_owned(),
            canonical_tool_paths_and_content_digests: BTreeMap::from([
                ("cargo".to_string(), tools.cargo_version.clone()),
                ("llvm-cov".to_string(), tools.llvm_cov_version.clone()),
                ("rustc".to_string(), tools.rustc_version.clone()),
                (
                    "cargo-nextest".to_string(),
                    tools.cargo_nextest_version.clone(),
                ),
            ]),
            runner_map_fingerprint: req.runner_map_fingerprint.clone(),
            normalized_child_environment: req.env.clone(),
            test_args:
                crate::rust_llvm_cov_runner::plan::batch_plan_test_args::identity_relevant_test_args(
                    &req.test_args,
                ),
            collection_policy_or_compile_context_digest: self.selection_context_fingerprint.clone(),
        }
    }

    pub fn source_snapshot(&self) -> SourceSnapshot {
        SourceSnapshot {
            schema_version: SOURCE_SNAPSHOT_SCHEMA_VERSION.to_string(),
            path_digests: self.ordinary_source_digests.clone(),
        }
    }

    pub fn timing_context(&self, req: &RustCoverageBatchRequest) -> TimingContextIdentity {
        TimingContextIdentity {
            schema_version: TIMING_CONTEXT_SCHEMA_VERSION.to_string(),
            host_platform: req.host_platform.clone(),
            runner_identity: req.runner_map_fingerprint.clone(),
            concurrency_and_jobs: req.jobs.to_string(),
            timeout_measurement_policy: CACHE_SCHEMA_VERSION.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust_llvm_cov_runner::test_support::witness_batch_tools;

    #[test]
    fn ordinary_source_digest_is_outside_execution_context() {
        let req = RustCoverageBatchRequest::witness();
        let tools = witness_batch_tools();
        let identity = RustCoverageBatchIdentity {
            input_digest: "global".into(),
            generation_fingerprint: "gen".into(),
            selection_context_fingerprint: "sel".into(),
            ordinary_source_digests: BTreeMap::from([("src/lib.rs".into(), "abc".into())]),
        };
        let ctx = identity.execution_context(&req, &tools);
        let snap = identity.source_snapshot();
        assert_eq!(ctx.language, "rust");
        assert_eq!(ctx.collection_policy_or_compile_context_digest, "sel");
        assert!(!snap.path_digests.is_empty());
        assert_ne!(format!("{ctx:?}"), format!("{:?}", snap.path_digests));
        let timing = identity.timing_context(&req);
        assert_eq!(timing.host_platform, req.host_platform);
    }
}
