use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use kiss::rust_llvm_cov_runner::RustCoverageBatchIdentity;

use crate::test_runner::execution_witness::{
    PublishRustWitness, WitnessScope, WitnessStatus, publish_rust_execution_witness,
};

static LIVE_WITNESS: Mutex<Option<LiveWitnessCache>> = Mutex::new(None);

const PERSIST_EVERY_N: usize = 64;
const PERSIST_EVERY: Duration = Duration::from_secs(2);

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
    dirty_events: usize,
    last_persist: Instant,
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
            dirty_events: 0,
            last_persist: Instant::now(),
        }
    }

    pub(crate) fn record_pass(&mut self, logical: &str, report: &str, duration: Duration) {
        let Some(idx) = self
            .selector_indices
            .get(logical)
            .or_else(|| self.selector_indices.get(report))
            .copied()
        else {
            return;
        };
        self.statuses[idx] = WitnessStatus::Passed;
        self.durations_ns[idx] = Some(duration.as_nanos() as u64);
        self.mark_dirty();
    }

    pub(crate) fn record_non_pass(&mut self, logical: &str, report: &str, status: WitnessStatus) {
        let Some(idx) = self
            .selector_indices
            .get(logical)
            .or_else(|| self.selector_indices.get(report))
            .copied()
        else {
            return;
        };
        self.statuses[idx] = status;
        self.mark_dirty();
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.dirty_events = self.dirty_events.saturating_add(1);
        if self.dirty_events >= PERSIST_EVERY_N || self.last_persist.elapsed() >= PERSIST_EVERY {
            self.persist();
        }
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
        self.dirty_events = 0;
        self.last_persist = Instant::now();
    }
}

pub(super) fn record_live_rust_pass(logical: &str, report: &str, duration: Duration) {
    if let Ok(mut guard) = LIVE_WITNESS.lock()
        && let Some(cache) = guard.as_mut()
    {
        cache.record_pass(logical, report, duration);
    }
}

pub(super) fn record_live_rust_non_pass(logical: &str, report: &str, status: WitnessStatus) {
    if let Ok(mut guard) = LIVE_WITNESS.lock()
        && let Some(cache) = guard.as_mut()
    {
        cache.record_non_pass(logical, report, status);
    }
}

pub(super) fn flush_live_rust_witness() {
    if let Ok(mut guard) = LIVE_WITNESS.lock()
        && let Some(mut cache) = guard.take()
    {
        cache.persist();
    }
}

#[cfg(test)]
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
