use std::collections::VecDeque;
use std::io;
use std::path::Path;
use std::sync::mpsc;

use crate::rust_llvm_cov_runner::{
    RustCovCacheStatus, RustLlvmCovError, RustLlvmCovOutcome,
    batch_fingerprint::{RustCoverageBatchIdentity, RustCoverageToolIdentity, entry_fingerprint},
    batch_plan::RustCoverageBatchRequest,
    rust_cov_cache::{RustCovCacheEntry, store_rust_cov_cache_entry},
};

pub(crate) fn store_completed_outcomes(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    completed: &mut [RustLlvmCovOutcome],
) -> Result<(), RustLlvmCovError> {
    store_completed_outcomes_with(req, tools, identity, completed, store_rust_cov_cache_entry)
}

#[derive(Debug)]
struct EntryStoreJob {
    outcome_index: usize,
    fingerprint: String,
    cache_entry: RustCovCacheEntry,
}

#[derive(Debug)]
enum EntryStoreResult {
    Success,
    Failure(io::Error),
}

struct EntryStoreDispatcher {
    jobs: VecDeque<EntryStoreJob>,
    results: Vec<Option<EntryStoreResult>>,
    worker_txs: Vec<mpsc::Sender<EntryStoreJob>>,
    result_rx: mpsc::Receiver<(usize, usize, EntryStoreResult)>,
    total_jobs: usize,
    next_job: usize,
    active: usize,
    stopped: bool,
}

impl EntryStoreDispatcher {
    fn new(
        jobs: VecDeque<EntryStoreJob>,
        worker_txs: Vec<mpsc::Sender<EntryStoreJob>>,
        result_rx: mpsc::Receiver<(usize, usize, EntryStoreResult)>,
    ) -> Self {
        let total_jobs = jobs.len();
        Self {
            jobs,
            results: std::iter::repeat_with(|| None).take(total_jobs).collect(),
            worker_txs,
            result_rx,
            total_jobs,
            next_job: 0,
            active: 0,
            stopped: false,
        }
    }

    fn run(
        mut self,
        worker_count: usize,
    ) -> Result<Vec<Option<EntryStoreResult>>, RustLlvmCovError> {
        for worker_index in 0..worker_count {
            self.dispatch_one(worker_index, "initial");
        }
        while self.active > 0 {
            self.receive_one_result()?;
        }
        drop(self.worker_txs);
        Ok(self.results)
    }

    fn dispatch_one(&mut self, worker_index: usize, dispatch_kind: &str) {
        self.worker_txs[worker_index]
            .send(self.jobs.pop_front().expect("entry store job"))
            .unwrap_or_else(|_| panic!("entry store workers should receive {dispatch_kind} jobs"));
        self.active += 1;
        self.next_job += 1;
    }

    fn receive_one_result(&mut self) -> Result<(), RustLlvmCovError> {
        let (worker_index, outcome_index, result) = self.result_rx.recv().map_err(|_| {
            RustLlvmCovError::Io(io::Error::other(
                "entry store worker exited without reporting result",
            ))
        })?;
        self.active -= 1;
        self.stopped |= matches!(result, EntryStoreResult::Failure(_));
        self.results[outcome_index] = Some(result);
        if !self.stopped && self.next_job < self.total_jobs {
            self.dispatch_one(worker_index, "replacement");
        }
        Ok(())
    }
}

pub(crate) fn store_completed_outcomes_with(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    completed: &mut [RustLlvmCovOutcome],
    store: impl Fn(&Path, &str, &RustCovCacheEntry) -> io::Result<()> + Sync,
) -> Result<(), RustLlvmCovError> {
    let jobs = entry_store_jobs(req, tools, identity, completed);
    let results = run_entry_store_jobs(req.jobs, &req.cache_root, jobs, store)?;
    reconcile_entry_store_results(completed, results)?;
    crate::rust_llvm_cov_runner::write_ordinary_source_snapshot(
        &req.cache_root,
        &req.source_root,
        identity,
    )
    .map_err(RustLlvmCovError::Io)
}

fn entry_store_jobs(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    completed: &[RustLlvmCovOutcome],
) -> VecDeque<EntryStoreJob> {
    completed
        .iter()
        .enumerate()
        .map(|(outcome_index, outcome)| EntryStoreJob {
            outcome_index,
            fingerprint: entry_fingerprint(&identity.input_digest, req, tools, &outcome.selector),
            cache_entry: RustCovCacheEntry::from_outcome(outcome, &identity.generation_fingerprint),
        })
        .collect()
}

fn run_entry_store_jobs(
    job_budget: usize,
    cache_root: &Path,
    jobs: VecDeque<EntryStoreJob>,
    store: impl Fn(&Path, &str, &RustCovCacheEntry) -> io::Result<()> + Sync,
) -> Result<Vec<Option<EntryStoreResult>>, RustLlvmCovError> {
    if jobs.is_empty() {
        return Ok(Vec::new());
    }
    let worker_count = job_budget.min(jobs.len());
    let (result_tx, result_rx) = mpsc::channel::<(usize, usize, EntryStoreResult)>();
    std::thread::scope(|scope| {
        let mut worker_txs = Vec::new();
        for worker_index in 0..worker_count {
            let (job_tx, job_rx) = mpsc::channel::<EntryStoreJob>();
            worker_txs.push(job_tx);
            let result_tx = result_tx.clone();
            let store = &store;
            scope.spawn(move || store_worker(worker_index, cache_root, job_rx, result_tx, store));
        }
        drop(result_tx);
        let results = EntryStoreDispatcher::new(jobs, worker_txs, result_rx).run(worker_count)?;
        sync_entries_dir(cache_root)?;
        Ok(results)
    })
}

fn sync_entries_dir(cache_root: &Path) -> Result<(), RustLlvmCovError> {
    let dir = cache_root.join("entries");
    match std::fs::File::open(&dir) {
        Ok(file) => file.sync_all().map_err(RustLlvmCovError::Io),
        Err(err)
            if err.kind() == io::ErrorKind::NotFound
                || err.kind() == io::ErrorKind::NotADirectory =>
        {
            Ok(())
        }
        Err(err) => Err(RustLlvmCovError::Io(err)),
    }
}

fn store_worker(
    worker_index: usize,
    cache_root: &Path,
    job_rx: mpsc::Receiver<EntryStoreJob>,
    result_tx: mpsc::Sender<(usize, usize, EntryStoreResult)>,
    store: &(impl Fn(&Path, &str, &RustCovCacheEntry) -> io::Result<()> + Sync),
) {
    while let Ok(job) = job_rx.recv() {
        let outcome_index = job.outcome_index;
        let result = match store(cache_root, &job.fingerprint, &job.cache_entry) {
            Ok(()) => EntryStoreResult::Success,
            Err(err) => EntryStoreResult::Failure(err),
        };
        if result_tx
            .send((worker_index, outcome_index, result))
            .is_err()
        {
            break;
        }
    }
}

fn reconcile_entry_store_results(
    completed: &mut [RustLlvmCovOutcome],
    results: Vec<Option<EntryStoreResult>>,
) -> Result<(), RustLlvmCovError> {
    let mut first_error = None;
    for (outcome_index, result) in results.into_iter().enumerate() {
        match result {
            Some(EntryStoreResult::Success) => {
                completed[outcome_index].cache_status = RustCovCacheStatus::MissStored;
            }
            Some(EntryStoreResult::Failure(err)) => {
                completed[outcome_index].cache_status = RustCovCacheStatus::FreshUnstored;
                first_error.get_or_insert(err);
            }
            None => {}
        }
    }
    first_error.map_or(Ok(()), |err| Err(RustLlvmCovError::Io(err)))
}

#[cfg(test)]
#[path = "batch_executor_finish_store_test.rs"]
mod tests;
