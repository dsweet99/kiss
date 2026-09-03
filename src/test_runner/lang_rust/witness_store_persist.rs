use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity;

use super::{
    OnDiskRustWitness, SCHEMA_VERSION, content_digest, covered_lines_for_disk, write_witness_atomic,
};
use crate::test_runner::lang_iface::WitnessStatus;

pub(super) struct PersistFullWitness<'a> {
    pub repo_root: &'a Path,
    pub identity: &'a RustCoverageBatchIdentity,
    pub identity_digest: &'a str,
    pub selectors: &'a [String],
    pub statuses: &'a [WitnessStatus],
    pub durations_ns: &'a [Option<u64>],
    pub covered_lines: &'a BTreeMap<String, BTreeSet<u32>>,
    pub complete: bool,
    pub jobs: usize,
}

impl PersistFullWitness<'_> {
    pub(super) fn persist(self) -> Result<String, String> {
        let timing = crate::test_runner::lang_iface::session_timing_context_digest(self.jobs);
        let complete_all_pass = self.complete
            && self
                .statuses
                .iter()
                .all(|status| *status == WitnessStatus::Passed);
        let generation_id = if complete_all_pass {
            super::super::generation_publish::publish_complete_full_generation(
                self.repo_root,
                self.identity_digest,
                self.selectors,
                self.statuses,
                self.durations_ns,
                &timing,
                &covered_lines_for_disk(self.covered_lines),
            )
        } else {
            super::super::generation_publish::publish_current_witness_generation(
                self.repo_root,
                self.identity_digest,
                self.selectors,
                self.statuses,
                self.durations_ns,
                &covered_lines_for_disk(self.covered_lines),
                super::super::generation_publish::WitnessGenerationState {
                    timing_context_digest: &timing,
                    complete: self.complete,
                },
            )
        }?;
        if !self.complete {
            let _ = self.write_legacy_json(&generation_id);
        }
        let _ = kiss::rust_llvm_cov_runner::write_ordinary_source_snapshot(
            &crate::test_runner::rust_coverage_index::rust_coverage_cache_root(self.repo_root),
            self.repo_root,
            self.identity,
        );
        Ok(generation_id)
    }

    fn write_legacy_json(&self, generation_id: &str) -> Result<(), String> {
        let mut body = OnDiskRustWitness {
            schema_version: SCHEMA_VERSION.to_string(),
            scope: "full".to_string(),
            identity_digest: self.identity_digest.to_string(),
            generation_id: generation_id.to_string(),
            complete: self.complete,
            selectors: self.selectors.to_vec(),
            statuses: self
                .statuses
                .iter()
                .map(|s| s.as_str().to_string())
                .collect(),
            durations_ns: self.durations_ns.to_vec(),
            covered_lines: covered_lines_for_disk(self.covered_lines),
            content_sha256: String::new(),
        };
        body.content_sha256 = content_digest(&body)?;
        write_witness_atomic(self.repo_root, &body)
    }
}
