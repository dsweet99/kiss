use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity;

use crate::test_runner::execution_witness::{
    PublishRustWitness, WitnessScope, WitnessStatus, publish_rust_execution_witness,
};

static LIVE_WITNESS: Mutex<Option<LiveWitnessCache>> = Mutex::new(None);

pub(crate) struct LiveWitnessCache {
    pub(crate) repo_root: PathBuf,
    pub(crate) identity: RustCoverageBatchIdentity,
    pub(crate) selectors: Vec<String>,
    pub(crate) statuses: Vec<WitnessStatus>,
    pub(crate) durations_ns: Vec<Option<u64>>,
    pub(crate) covered_lines: BTreeMap<String, BTreeSet<u32>>,
    pub(crate) selector_indices: BTreeMap<String, usize>,
    pub(crate) dirty: bool,
    pub(crate) jobs: usize,
}

impl LiveWitnessCache {
    pub(crate) fn new(
        repo_root: &Path,
        batch_identity: &RustCoverageBatchIdentity,
        population_selectors: Option<&[String]>,
        fallback_selectors: &[String],
        jobs: usize,
    ) -> Self {
        let existing = super::witness::load_matching_full_witness(repo_root, batch_identity);
        let mut universe: Vec<String> = match population_selectors {
            Some(pop) => pop.to_vec(),
            None => fallback_selectors.to_vec(),
        };
        if let Some(existing) = existing.as_ref() {
            for sel in &existing.selectors {
                if !universe.contains(sel) {
                    universe.push(sel.clone());
                }
            }
        }
        for sel in fallback_selectors {
            if !universe.contains(sel) {
                universe.push(sel.clone());
            }
        }
        universe.sort();
        universe.dedup();

        let mut statuses = vec![WitnessStatus::Unresolved; universe.len()];
        let mut durations_ns = vec![None; universe.len()];
        let mut covered_lines = BTreeMap::new();

        if let Some(existing) = existing.as_ref() {
            let existing_idx: BTreeMap<&str, usize> = existing
                .selectors
                .iter()
                .enumerate()
                .map(|(i, s)| (s.as_str(), i))
                .collect();
            for (i, sel) in universe.iter().enumerate() {
                if let Some(&ei) = existing_idx.get(sel.as_str()) {
                    statuses[i] = existing.statuses[ei];
                    durations_ns[i] = existing.durations_ns[ei];
                }
            }
            for (path, lines) in &existing.covered_lines {
                covered_lines.insert(path.clone(), lines.iter().copied().collect());
            }
        }

        let selector_indices: BTreeMap<String, usize> = universe
            .iter()
            .enumerate()
            .map(|(i, s)| (s.clone(), i))
            .collect();

        Self {
            repo_root: repo_root.to_path_buf(),
            identity: batch_identity.clone(),
            selectors: universe,
            statuses,
            durations_ns,
            covered_lines,
            selector_indices,
            dirty: false,
            jobs,
        }
    }

    pub(crate) fn record_pass(&mut self, logical: &str, report: &str, duration: Duration) {
        let idx = self
            .selector_indices
            .get(logical)
            .or_else(|| self.selector_indices.get(report))
            .copied()
            .unwrap_or_else(|| {
                let i = self.selectors.len();
                self.selectors.push(logical.to_string());
                self.statuses.push(WitnessStatus::Unresolved);
                self.durations_ns.push(None);
                self.selector_indices.insert(logical.to_string(), i);
                i
            });
        self.statuses[idx] = WitnessStatus::Passed;
        self.durations_ns[idx] = Some(duration.as_nanos() as u64);
        self.dirty = true;
    }

    pub(crate) fn record_non_pass(&mut self, logical: &str, report: &str, status: WitnessStatus) {
        let idx = self
            .selector_indices
            .get(logical)
            .or_else(|| self.selector_indices.get(report))
            .copied()
            .unwrap_or_else(|| {
                let i = self.selectors.len();
                self.selectors.push(logical.to_string());
                self.statuses.push(WitnessStatus::Unresolved);
                self.durations_ns.push(None);
                self.selector_indices.insert(logical.to_string(), i);
                i
            });
        self.statuses[idx] = status;
        self.dirty = true;
    }

    pub(crate) fn persist(&mut self) {
        if !self.dirty {
            return;
        }
        let complete = self
            .statuses
            .iter()
            .all(|status| *status == WitnessStatus::Passed);
        let _ = publish_rust_execution_witness(PublishRustWitness {
            repo_root: &self.repo_root,
            identity: &self.identity,
            scope: WitnessScope::Full,
            selectors: &self.selectors,
            statuses: &self.statuses,
            durations_ns: &self.durations_ns,
            covered_lines: &self.covered_lines,
            complete,
            jobs: self.jobs,
        });
        self.dirty = false;
    }
}

pub(super) fn record_live_rust_pass(logical: &str, report: &str, duration: Duration) {
    if let Ok(mut guard) = LIVE_WITNESS.lock()
        && let Some(cache) = guard.as_mut()
    {
        cache.record_pass(logical, report, duration);
        cache.persist();
    }
}

pub(super) fn record_live_rust_non_pass(logical: &str, report: &str, status: WitnessStatus) {
    if let Ok(mut guard) = LIVE_WITNESS.lock()
        && let Some(cache) = guard.as_mut()
    {
        cache.record_non_pass(logical, report, status);
        cache.persist();
    }
}

pub(super) fn flush_live_rust_witness() {
    if let Ok(mut guard) = LIVE_WITNESS.lock()
        && let Some(mut cache) = guard.take()
    {
        cache.persist();
    }
}

pub(super) fn clear_live_rust_witness() {
    if let Ok(mut guard) = LIVE_WITNESS.lock() {
        *guard = None;
    }
}

pub(super) fn seed_live_witness_cache(cache: LiveWitnessCache) {
    if let Ok(mut guard) = LIVE_WITNESS.lock() {
        *guard = Some(cache);
    }
}

