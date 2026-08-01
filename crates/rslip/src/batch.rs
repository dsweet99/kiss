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

impl Rslip {
    pub fn run_or_reuse_many_bounded(
        &self,
        reqs: Vec<RslipRequest>,
        jobs: usize,
    ) -> Vec<Result<RslipOutcome, RslipError>> {
        assert!(jobs > 0, "jobs must be greater than zero");
        let mut out = Vec::new();
        out.resize_with(reqs.len(), || None);
        let mut misses = Vec::new();
        let shared_context = reqs.first().and_then(|first| {
            if let Some(cached) = crate::batch_context_seal::try_batch_context_seal(first) {
                return Some(cached);
            }
            let context = rslip_request_context_fingerprint(first).ok()?;
            let _ = crate::batch_context_seal::write_batch_context_seal(first, &context);
            Some(context)
        });

        for (index, req) in reqs.into_iter().enumerate() {
            match prepare_rslip_cache_candidate(index, req, shared_context.as_deref()) {
                Ok(candidate) => {
                    if !candidate.req.force_rerun
                        && let Some(entry) = load_rslip_cache_entry(
                            &candidate.req.cache_root,
                            &candidate.fingerprint,
                        )
                    {
                        out[index] = Some(Ok(rslip_outcome_from_cache(entry)));
                    } else {
                        misses.push(candidate);
                    }
                }
                Err(err) => out[index] = Some(Err(err)),
            }
        }

        let LockedRslipMisses {
            misses: runner_misses,
            _guards: _entry_guards,
        } = prepare_rslip_misses(misses, &mut out);
        if runner_misses.is_empty() {
            return finalize_rslip_batch_results(out);
        }
        let runner_reqs: Vec<_> = runner_misses
            .iter()
            .map(|miss| miss.runner_req.clone())
            .collect();
        let runner_outcomes = self.runner.run_many_bounded(runner_reqs, jobs);

        for (miss, result) in runner_misses.into_iter().zip(runner_outcomes) {
            for (index, result) in handle_rslip_miss_result(miss, result) {
                out[index] = Some(result);
            }
        }

        finalize_rslip_batch_results(out)
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
    misses: Vec<RslipCacheCandidate>,
    out: &mut [Option<Result<RslipOutcome, RslipError>>],
) -> LockedRslipMisses {
    let mut guards = Vec::new();
    let misses = lock_and_filter_rslip_misses(misses, out, &mut guards);
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

fn lock_and_filter_rslip_misses(
    misses: Vec<RslipCacheCandidate>,
    out: &mut [Option<Result<RslipOutcome, RslipError>>],
    guards: &mut Vec<crate::LocalRslipLockGuard>,
) -> Vec<RslipCacheCandidateGroup> {
    let mut groups: BTreeMap<(PathBuf, String), Vec<RslipCacheCandidate>> = BTreeMap::new();
    for miss in misses {
        groups
            .entry((miss.canonical_cache_root.clone(), miss.fingerprint.clone()))
            .or_default()
            .push(miss);
    }
    let mut runner_groups = Vec::new();
    for ((_canonical_root, fingerprint), candidates) in groups {
        let first = candidates
            .first()
            .expect("rslip miss group contains at least one candidate");
        match crate::lock_rslip_cache_entry(&first.req.cache_root, &fingerprint) {
            Ok(guard) => {
                let force_rerun = candidates.iter().any(|candidate| candidate.req.force_rerun);
                if !force_rerun
                    && let Some(entry) = load_rslip_cache_entry(&first.req.cache_root, &fingerprint)
                {
                    for candidate in candidates {
                        out[candidate.index] = Some(Ok(rslip_outcome_from_cache(entry.clone())));
                    }
                } else {
                    guards.push(guard);
                    runner_groups.push(RslipCacheCandidateGroup {
                        indices: candidates.iter().map(|candidate| candidate.index).collect(),
                        representative: candidates
                            .into_iter()
                            .next()
                            .expect("rslip miss group contains a representative"),
                        fingerprint,
                    });
                }
            }
            Err(err) => {
                for candidate in candidates {
                    out[candidate.index] = Some(Err(RslipError::Io(io::Error::new(
                        err.kind(),
                        err.to_string(),
                    ))));
                }
            }
        }
    }
    runner_groups.sort_by_key(|group| group.indices.first().copied().unwrap_or(usize::MAX));
    runner_groups
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
    let outcome = result?;
    let coverage = rslip_coverage_from_outcome(&outcome)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use rpytest_runner::RequestedArtifact;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    #[test]
    fn rslip_cache_candidate_and_miss_store_batch_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let req = crate::rslip_sample_request(tmp.path());
        let runner_req = PytestRunRequest {
            nodeid: req.nodeid.clone(),
            cwd: req.cwd.clone(),
            python: req.python.clone(),
            pytest_args: req.pytest_args.clone(),
            env: BTreeMap::new(),
            child_preload_modules: Vec::new(),
            artifacts: vec![RequestedArtifact {
                name: "coverage".to_string(),
                path: PathBuf::from("coverage.json"),
            }],
            timeout: None,
        };

        let candidate = RslipCacheCandidate {
            index: 3,
            req: req.clone(),
            fingerprint: "abc".to_string(),
            canonical_cache_root: req.cache_root.clone(),
        };
        let miss = RslipMiss {
            indices: vec![candidate.index],
            req: candidate.req,
            fingerprint: candidate.fingerprint,
            runner_req,
        };

        assert_eq!(miss.indices, vec![3]);
        assert_eq!(miss.req.nodeid, req.nodeid);
        assert_eq!(miss.fingerprint, "abc");
        assert_eq!(miss.runner_req.artifacts[0].name, "coverage");
    }

    #[test]
    fn locked_rslip_misses_and_candidate_group_types_are_test_referenced() {
        assert!(std::any::type_name::<LockedRslipMisses>().contains("LockedRslipMisses"));
        assert!(
            std::any::type_name::<RslipCacheCandidateGroup>().contains("RslipCacheCandidateGroup")
        );
    }
}
