use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rpytest_runner::PytestRunRequest;

use crate::cache::{
    DigestMemo, load_reusable_rslip_cache_entry_with_memo, rslip_cache_fingerprint,
    rslip_cache_fingerprint_from_context, rslip_request_context_fingerprint, rslip_unique_suffix,
};
use crate::{
    Rslip, RslipError, RslipOutcome, RslipRequest, build_pytest_runner_request,
    rslip_outcome_from_cache, runtime, validate_rslip_request,
};

mod cached_status;
mod finalize;
mod lock_chunk;
mod miss_run;
mod pycache;
mod warm_hit_seal;
use cached_status::{emit_prepare_resolved_progress, format_cached_status_dump};
use finalize::clone_rslip_error;
use miss_run::run_rslip_misses;
pub use warm_hit_seal::warm_hit_seal_exists;

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

pub(crate) struct PreparedRslipMisses {
    pub(crate) misses: Vec<RslipMiss>,
}

enum PublishedRuntime {
    Ready(PathBuf),
    Error(io::ErrorKind, String),
}

#[derive(Debug)]
pub enum RslipBatchProgress {
    Prepared {
        cache_hits: usize,
        cache_misses: usize,
        elapsed: Duration,
    },
    SelectorFinalized {
        outcomes: Vec<(usize, Result<RslipOutcome, RslipError>)>,
    },
    CachedStatusDump {
        body: String,
    },
    TestsRemaining {
        remaining: usize,
    },
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
        let prepare_started = Instant::now();
        if let Some(sealed) = try_prepare_from_warm_hit_seal(&reqs) {
            let cache_hits = sealed.len();
            on_progress(RslipBatchProgress::Prepared {
                cache_hits,
                cache_misses: 0,
                elapsed: prepare_started.elapsed(),
            });
            on_progress(RslipBatchProgress::CachedStatusDump {
                body: format_cached_status_dump(&sealed),
            });
            let out: Vec<Option<Result<RslipOutcome, RslipError>>> = sealed
                .into_iter()
                .map(|outcome| Some(Ok(outcome)))
                .collect();
            return finalize_rslip_batch_results(out);
        }
        let context_fingerprint = shared_batch_context(&reqs);
        let cache_root = reqs.first().map(|req| req.cache_root.clone());
        let source_root = reqs.first().map(|req| req.source_root.clone());
        let content_fingerprint = reqs.first().and_then(|req| req.content_fingerprint.clone());
        let (mut out, misses, digest_union) = prepare_rslip_batch_slots(reqs);
        let cache_misses = misses.len();
        if cache_misses == 0 {
            if let (Some(context), Some(cache_root), Some(source_root)) =
                (context_fingerprint.as_deref(), cache_root, source_root)
            {
                maybe_write_warm_hit_seal_from_hits(
                    cache_root,
                    source_root,
                    context,
                    &out,
                    digest_union,
                    content_fingerprint,
                );
            }
            on_progress(RslipBatchProgress::Prepared {
                cache_hits: out.iter().filter(|slot| slot.is_some()).count(),
                cache_misses,
                elapsed: prepare_started.elapsed(),
            });
            emit_prepare_resolved_progress(&out, &mut on_progress);
            return finalize_rslip_batch_results(out);
        }
        on_progress(RslipBatchProgress::Prepared {
            cache_hits: out.iter().filter(|slot| slot.is_some()).count(),
            cache_misses,
            elapsed: prepare_started.elapsed(),
        });
        emit_prepare_resolved_progress(&out, &mut on_progress);
        run_rslip_misses(self, misses, cache_misses, jobs, &mut out, &mut on_progress);
        finalize_rslip_batch_results(out)
    }
}

fn try_prepare_from_warm_hit_seal(reqs: &[RslipRequest]) -> Option<Vec<RslipOutcome>> {
    let context = shared_batch_context(reqs)?;
    warm_hit_seal::try_warm_hit_seal(reqs, &context)
}

fn maybe_write_warm_hit_seal_from_hits(
    cache_root: PathBuf,
    source_root: PathBuf,
    context_fingerprint: &str,
    out: &[Option<Result<RslipOutcome, RslipError>>],
    digest_union: BTreeMap<String, String>,
    content_fingerprint: Option<String>,
) {
    if digest_union.is_empty() {
        return;
    }
    let mut nodeids = Vec::with_capacity(out.len());
    let mut outcomes = Vec::with_capacity(out.len());
    for slot in out {
        let Some(Ok(outcome)) = slot else {
            return;
        };
        if outcome.cache_status != crate::CacheStatus::Hit {
            return;
        }
        nodeids.push(outcome.nodeid.clone());
        outcomes.push(RslipOutcome {
            nodeid: outcome.nodeid.clone(),
            status: outcome.status,
            exit_code: outcome.exit_code,
            duration: outcome.duration,
            coverage: crate::LineCoverage {
                files: BTreeMap::new(),
            },
            cache_status: outcome.cache_status,
            stdout: None,
            stderr: None,
        });
    }
    let _ = warm_hit_seal::write_warm_hit_seal(
        &cache_root,
        &source_root,
        context_fingerprint,
        &nodeids,
        &outcomes,
        digest_union,
        content_fingerprint,
    );
}

fn shared_batch_context(reqs: &[RslipRequest]) -> Option<String> {
    reqs.first()
        .and_then(|first| rslip_request_context_fingerprint(first).ok())
}

type PreparedBatchSlots = (
    Vec<Option<Result<RslipOutcome, RslipError>>>,
    Vec<RslipCacheCandidate>,
    BTreeMap<String, String>,
);

fn prepare_rslip_batch_slots(reqs: Vec<RslipRequest>) -> PreparedBatchSlots {
    let mut out = Vec::new();
    out.resize_with(reqs.len(), || None);
    let mut misses = Vec::new();
    let mut digest_memo = DigestMemo::new();
    let mut digest_union = BTreeMap::new();
    let shared_context = shared_batch_context(&reqs);
    let shared_canonical_cache_root = reqs.first().and_then(|req| {
        let _ = fs::create_dir_all(&req.cache_root);
        req.cache_root.canonicalize().ok()
    });
    for (index, req) in reqs.into_iter().enumerate() {
        match prepare_rslip_cache_candidate(
            index,
            req,
            shared_context.as_deref(),
            shared_canonical_cache_root.as_deref(),
        ) {
            Ok(candidate) => {
                if !candidate.req.force_rerun
                    && let Some(entry) = load_reusable_rslip_cache_entry_with_memo(
                        &candidate.req.cache_root,
                        &candidate.fingerprint,
                        &candidate.req.source_root,
                        &mut digest_memo,
                    )
                {
                    for (path, digest) in &entry.covered_digests {
                        digest_union.insert(path.clone(), digest.clone());
                    }
                    out[index] = Some(Ok(rslip_outcome_from_cache(entry)));
                } else {
                    misses.push(candidate);
                }
            }
            Err(err) => out[index] = Some(Err(err)),
        }
    }
    (out, misses, digest_union)
}

fn prepare_rslip_cache_candidate(
    index: usize,
    req: RslipRequest,
    shared_context: Option<&str>,
    shared_canonical_cache_root: Option<&Path>,
) -> Result<RslipCacheCandidate, RslipError> {
    validate_rslip_request(&req)?;
    let canonical_cache_root = if let Some(shared) = shared_canonical_cache_root {
        shared.to_path_buf()
    } else {
        fs::create_dir_all(&req.cache_root)?;
        req.cache_root.canonicalize()?
    };
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

pub(super) fn prepare_rslip_misses(
    groups: Vec<RslipCacheCandidateGroup>,
    out: &mut [Option<Result<RslipOutcome, RslipError>>],
) -> PreparedRslipMisses {
    let mut roots = BTreeSet::new();
    for miss in &groups {
        roots.insert(miss.representative.req.cache_root.clone());
    }
    let runtimes = publish_rslip_runtimes(roots);
    let mut runner_misses = Vec::new();
    for miss in groups {
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
    PreparedRslipMisses {
        misses: runner_misses,
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
