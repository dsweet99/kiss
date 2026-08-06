use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use rpytest_runner::{PytestRunError, PytestRunOutcome, PytestRunRequest};

use crate::cache::{
    RslipCacheEntry, load_rslip_cache_entry, rslip_cache_fingerprint,
    rslip_cache_fingerprint_from_context, rslip_request_context_fingerprint, rslip_unique_suffix,
    store_rslip_cache_entry,
};
use crate::{
    CacheStatus, Rslip, RslipError, RslipOutcome, RslipRequest, build_pytest_runner_request,
    rslip_coverage_from_outcome, rslip_outcome_from_cache, runtime, validate_rslip_request,
};

mod lock_chunk;
pub(crate) use lock_chunk::rslip_entry_lock_chunk_size;
use lock_chunk::{coalesce_rslip_miss_candidates, lock_and_filter_rslip_miss_groups};

pub(crate) struct RslipCacheCandidate {
    pub(crate) index: usize,
    pub(crate) req: RslipRequest,
    pub(crate) fingerprint: String,
    pub(crate) canonical_cache_root: PathBuf,
}

pub(crate) struct RslipMiss {
    pub(crate) indices: Vec<usize>,
    pub(crate) req: RslipRequest,
    pub(crate) fingerprint: String,
    pub(crate) runner_req: PytestRunRequest,
}

pub(crate) struct LockedRslipMisses {
    pub(crate) misses: Vec<RslipMiss>,
    pub(crate) _guards: Vec<crate::LocalRslipLockGuard>,
}

enum PublishedRuntime {
    Ready(PathBuf),
    Error(io::ErrorKind, String),
}

/// Progress events emitted while a bounded rslip batch runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RslipBatchProgress {
    /// Cache prepare finished; misses still need pytest execution.
    Prepared { cache_hits: usize, cache_misses: usize },
    /// One request resolved (cache hit, miss store, or error).
    Resolved { remaining_misses: usize },
}

impl Rslip {
    pub fn run_or_reuse_many_bounded(
        &self,
        reqs: Vec<RslipRequest>,
        jobs: usize,
    ) -> Vec<Result<RslipOutcome, RslipError>> {
        self.run_or_reuse_many_bounded_with_progress(reqs, jobs, |_| {})
    }

    pub fn run_or_reuse_many_bounded_with_progress(
        &self,
        reqs: Vec<RslipRequest>,
        jobs: usize,
        mut on_progress: impl FnMut(RslipBatchProgress),
    ) -> Vec<Result<RslipOutcome, RslipError>> {
        assert!(jobs > 0, "jobs must be greater than zero");
        let (mut out, misses) = prepare_rslip_batch_slots(reqs);
        let cache_misses = misses.len();
        on_progress(RslipBatchProgress::Prepared {
            cache_hits: out.iter().filter(|slot| slot.is_some()).count(),
            cache_misses,
        });
        run_rslip_miss_chunks(self, misses, cache_misses, jobs, &mut out, &mut on_progress);
        finalize_rslip_batch_results(out)
    }
}

fn shared_batch_context(reqs: &[RslipRequest]) -> Option<String> {
    reqs.first().and_then(|first| {
        if let Some(cached) = crate::batch_context_seal::try_batch_context_seal(first) {
            return Some(cached);
        }
        let context = rslip_request_context_fingerprint(first).ok()?;
        let _ = crate::batch_context_seal::write_batch_context_seal(first, &context);
        Some(context)
    })
}

fn prepare_rslip_batch_slots(
    reqs: Vec<RslipRequest>,
) -> (
    Vec<Option<Result<RslipOutcome, RslipError>>>,
    Vec<RslipCacheCandidate>,
) {
    let mut out = Vec::new();
    out.resize_with(reqs.len(), || None);
    let mut misses = Vec::new();
    let shared_context = shared_batch_context(&reqs);
    for (index, req) in reqs.into_iter().enumerate() {
        match prepare_rslip_cache_candidate(index, req, shared_context.as_deref()) {
            Ok(candidate) => {
                if !candidate.req.force_rerun
                    && let Some(entry) =
                        load_rslip_cache_entry(&candidate.req.cache_root, &candidate.fingerprint)
                {
                    out[index] = Some(Ok(rslip_outcome_from_cache(entry)));
                } else {
                    misses.push(candidate);
                }
            }
            Err(err) => out[index] = Some(Err(err)),
        }
    }
    (out, misses)
}

fn run_rslip_miss_chunks(
    rslip: &Rslip,
    misses: Vec<RslipCacheCandidate>,
    mut remaining_misses: usize,
    jobs: usize,
    out: &mut [Option<Result<RslipOutcome, RslipError>>],
    on_progress: &mut impl FnMut(RslipBatchProgress),
) {
    // Coalesce duplicates, then lock/run in FD-bounded chunks (avoids EMFILE).
    let mut groups = coalesce_rslip_miss_candidates(misses);
    let chunk_size = rslip_entry_lock_chunk_size(jobs);
    while !groups.is_empty() {
        let take = chunk_size.min(groups.len());
        let chunk: Vec<_> = groups.drain(..take).collect();
        let LockedRslipMisses {
            misses: runner_misses,
            _guards: _entry_guards,
        } = prepare_rslip_misses(chunk, out);
        if runner_misses.is_empty() {
            continue;
        }
        let runner_reqs: Vec<_> = runner_misses
            .iter()
            .map(|miss| miss.runner_req.clone())
            .collect();
        let runner_outcomes = rslip.runner.run_many_bounded(runner_reqs, jobs);
        for (miss, result) in runner_misses.into_iter().zip(runner_outcomes) {
            let resolved = miss.indices.len();
            for (index, result) in handle_rslip_miss_result(miss, result) {
                out[index] = Some(result);
            }
            remaining_misses = remaining_misses.saturating_sub(resolved);
            on_progress(RslipBatchProgress::Resolved { remaining_misses });
        }
    }
}

fn prepare_rslip_cache_candidate(
    index: usize,
    req: RslipRequest,
    shared_context: Option<&str>,
) -> Result<RslipCacheCandidate, RslipError> {
    validate_rslip_request(&req)?;
    fs::create_dir_all(&req.cache_root)?;
    let canonical_cache_root = req.cache_root.canonicalize()?;
    let fingerprint = match shared_context {
        Some(context) => rslip_cache_fingerprint_from_context(context, &req.nodeid),
        None => rslip_cache_fingerprint(&req)?,
    };
    Ok(RslipCacheCandidate {
        index,
        req,
        fingerprint,
        canonical_cache_root,
    })
}

fn prepare_rslip_misses(
    groups: Vec<RslipCacheCandidateGroup>,
    out: &mut [Option<Result<RslipOutcome, RslipError>>],
) -> LockedRslipMisses {
    let mut guards = Vec::new();
    let misses = lock_and_filter_rslip_miss_groups(groups, out, &mut guards);
    let mut roots = BTreeSet::new();
    for miss in &misses {
        roots.insert(miss.representative.req.cache_root.clone());
    }
    let runtimes = publish_rslip_runtimes(roots);
    let mut runner_misses = Vec::new();
    for miss in misses {
        let runtime = runtimes
            .get(&miss.representative.req.cache_root)
            .expect("runtime publication attempted for every miss root");
        let runtime_dir = match runtime {
            PublishedRuntime::Ready(path) => path,
            PublishedRuntime::Error(kind, message) => {
                for index in miss.indices {
                    out[index] = Some(Err(RslipError::Io(io::Error::new(*kind, message.clone()))));
                }
                continue;
            }
        };
        match prepare_rslip_runner_miss(miss, runtime_dir) {
            Ok(runner_miss) => runner_misses.push(runner_miss),
            Err((indices, err)) => {
                for index in indices {
                    out[index] = Some(Err(clone_rslip_error(&err)));
                }
            }
        }
    }
    LockedRslipMisses {
        misses: runner_misses,
        _guards: guards,
    }
}

pub(crate) struct RslipCacheCandidateGroup {
    pub(crate) indices: Vec<usize>,
    pub(crate) representative: RslipCacheCandidate,
    pub(crate) fingerprint: String,
}

fn publish_rslip_runtimes(cache_roots: BTreeSet<PathBuf>) -> BTreeMap<PathBuf, PublishedRuntime> {
    let mut runtime_dirs = BTreeMap::new();
    for cache_root in cache_roots {
        let published = match publish_rslip_runtime(&cache_root) {
            Ok(runtime_dir) => PublishedRuntime::Ready(runtime_dir),
            Err(err) => PublishedRuntime::Error(err.kind(), err.to_string()),
        };
        runtime_dirs.insert(cache_root, published);
    }
    runtime_dirs
}

fn finalize_rslip_batch_results(
    out: Vec<Option<Result<RslipOutcome, RslipError>>>,
) -> Vec<Result<RslipOutcome, RslipError>> {
    out.into_iter()
        .map(|result| {
            result.unwrap_or_else(|| {
                Err(RslipError::InvalidRequest(
                    "rslip batch did not produce a result".to_string(),
                ))
            })
        })
        .collect()
}

fn prepare_rslip_runner_miss(
    miss: RslipCacheCandidateGroup,
    runtime_dir: &Path,
) -> Result<RslipMiss, (Vec<usize>, RslipError)> {
    let req = miss.representative.req;
    fs::create_dir_all(req.cache_root.join("testmon"))
        .map_err(|err| (miss.indices.clone(), RslipError::Io(err)))?;
    let artifact_path = req.cache_root.join("artifacts").join(format!(
        "{}.{}.json",
        miss.fingerprint,
        rslip_unique_suffix()
    ));
    if let Some(parent) = artifact_path.parent() {
        fs::create_dir_all(parent).map_err(|err| (miss.indices.clone(), RslipError::Io(err)))?;
    }
    let runner_req = build_pytest_runner_request(&req, runtime_dir, &artifact_path);
    Ok(RslipMiss {
        indices: miss.indices,
        req,
        fingerprint: miss.fingerprint,
        runner_req,
    })
}

fn publish_rslip_runtime(cache_root: &Path) -> io::Result<PathBuf> {
    let run_dir = cache_root.join("runtime");
    fs::create_dir_all(&run_dir)?;
    let runtime_path = run_dir.join(format!("{}.py", runtime::MODULE_NAME));
    let tmp_path = run_dir.join(format!(
        ".{}.{}.tmp",
        runtime::MODULE_NAME,
        rslip_unique_suffix()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)?;
    file.write_all(runtime::PYTHON_RUNTIME.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp_path, runtime_path)?;
    Ok(run_dir)
}

fn handle_rslip_miss_result(
    miss: RslipMiss,
    result: Result<PytestRunOutcome, PytestRunError>,
) -> Vec<(usize, Result<RslipOutcome, RslipError>)> {
    let result = handle_rslip_miss_result_once(&miss, result);
    miss.indices
        .into_iter()
        .map(|index| (index, clone_rslip_result(&result)))
        .collect()
}

fn handle_rslip_miss_result_once(
    miss: &RslipMiss,
    result: Result<PytestRunOutcome, PytestRunError>,
) -> Result<RslipOutcome, RslipError> {
    let outcome = match result {
        Ok(outcome) => outcome,
        // Timeouts are recorded as failed outcomes (empty coverage) so a large
        // population can finish instead of hanging forever on one stuck test.
        Err(PytestRunError::Timeout(timeout)) => {
            return store_failed_miss_outcome(
                miss,
                timeout,
                124,
                format!("rslip: pytest timed out after {timeout:?}\n"),
            );
        }
        Err(err) => return Err(RslipError::Runner(err)),
    };
    let coverage = match rslip_coverage_from_outcome(&outcome) {
        Ok(coverage) => coverage,
        // Missing coverage must not abort a multi-thousand-selector population.
        Err(RslipError::MissingArtifact(name)) => {
            return store_failed_miss_outcome(
                miss,
                outcome.duration,
                outcome.exit_code.unwrap_or(1),
                format!("rslip: missing coverage artifact {name}\n"),
            );
        }
        Err(err) => return Err(err),
    };
    let rslip_outcome = RslipOutcome {
        nodeid: outcome.nodeid,
        status: outcome.status,
        exit_code: outcome.exit_code,
        duration: outcome.duration,
        coverage,
        cache_status: CacheStatus::MissStored,
        stdout: Some(outcome.stdout),
        stderr: Some(outcome.stderr),
    };
    store_rslip_cache_entry(
        &miss.req.cache_root,
        &miss.fingerprint,
        &RslipCacheEntry::from(&rslip_outcome),
    )?;
    Ok(rslip_outcome)
}

fn store_failed_miss_outcome(
    miss: &RslipMiss,
    duration: std::time::Duration,
    exit_code: i32,
    stderr: String,
) -> Result<RslipOutcome, RslipError> {
    let rslip_outcome = RslipOutcome {
        nodeid: miss.req.nodeid.clone(),
        status: rpytest_runner::TestStatus::Failed,
        exit_code: Some(exit_code),
        duration,
        coverage: crate::LineCoverage {
            files: std::collections::BTreeMap::new(),
        },
        cache_status: CacheStatus::MissStored,
        stdout: None,
        stderr: Some(stderr.into_bytes()),
    };
    store_rslip_cache_entry(
        &miss.req.cache_root,
        &miss.fingerprint,
        &RslipCacheEntry::from(&rslip_outcome),
    )?;
    Ok(rslip_outcome)
}

fn clone_rslip_result(
    result: &Result<RslipOutcome, RslipError>,
) -> Result<RslipOutcome, RslipError> {
    match result {
        Ok(outcome) => Ok(outcome.clone()),
        Err(err) => Err(clone_rslip_error(err)),
    }
}

fn clone_rslip_error(err: &RslipError) -> RslipError {
    match err {
        RslipError::Io(err) => RslipError::Io(io::Error::new(err.kind(), err.to_string())),
        RslipError::Json(err) => {
            RslipError::Json(serde_json::Error::io(io::Error::other(err.to_string())))
        }
        RslipError::Runner(err) => RslipError::Runner(clone_pytest_error(err)),
        RslipError::MissingArtifact(name) => RslipError::MissingArtifact(name.clone()),
        RslipError::InvalidRequest(message) => RslipError::InvalidRequest(message.clone()),
    }
}

fn clone_pytest_error(err: &PytestRunError) -> PytestRunError {
    match err {
        PytestRunError::InvalidRequest(message) => PytestRunError::InvalidRequest(message.clone()),
        PytestRunError::Protocol(message) => PytestRunError::Protocol(message.clone()),
        PytestRunError::Spawn { program, message } => PytestRunError::Spawn {
            program: program.clone(),
            message: message.clone(),
        },
        PytestRunError::Timeout(timeout) => PytestRunError::Timeout(*timeout),
        PytestRunError::WorkerPanic => PytestRunError::WorkerPanic,
    }
}
