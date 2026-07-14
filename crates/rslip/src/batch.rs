use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use rpytest_runner::{PytestRunError, PytestRunOutcome, PytestRunRequest};

use crate::cache::{
    RslipCacheEntry, load_rslip_cache_entry, rslip_cache_fingerprint, rslip_unique_suffix,
    store_rslip_cache_entry,
};
use crate::{
    CacheStatus, Rslip, RslipError, RslipOutcome, RslipRequest, build_pytest_runner_request,
    rslip_coverage_from_outcome, rslip_outcome_from_cache, runtime, validate_rslip_request,
};

struct RslipCacheCandidate {
    index: usize,
    req: RslipRequest,
    fingerprint: String,
}

struct RslipMiss {
    index: usize,
    req: RslipRequest,
    fingerprint: String,
    runner_req: PytestRunRequest,
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

        for (index, req) in reqs.into_iter().enumerate() {
            match prepare_rslip_cache_candidate(index, req) {
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

        let runner_misses = prepare_rslip_misses(misses, &mut out);
        if runner_misses.is_empty() {
            return finalize_rslip_batch_results(out);
        }
        let runner_reqs: Vec<_> = runner_misses
            .iter()
            .map(|miss| miss.runner_req.clone())
            .collect();
        let runner_outcomes = self.runner.run_many_bounded(runner_reqs, jobs);

        for (miss, result) in runner_misses.into_iter().zip(runner_outcomes) {
            let index = miss.index;
            out[index] = Some(handle_rslip_miss_result(miss, result));
        }

        finalize_rslip_batch_results(out)
    }
}

fn prepare_rslip_cache_candidate(
    index: usize,
    req: RslipRequest,
) -> Result<RslipCacheCandidate, RslipError> {
    validate_rslip_request(&req)?;
    fs::create_dir_all(&req.cache_root)?;
    let fingerprint = rslip_cache_fingerprint(&req)?;
    Ok(RslipCacheCandidate {
        index,
        req,
        fingerprint,
    })
}

fn prepare_rslip_misses(
    misses: Vec<RslipCacheCandidate>,
    out: &mut [Option<Result<RslipOutcome, RslipError>>],
) -> Vec<RslipMiss> {
    let mut roots = BTreeSet::new();
    for miss in &misses {
        roots.insert(miss.req.cache_root.clone());
    }
    let runtimes = publish_rslip_runtimes(roots);
    let mut runner_misses = Vec::new();
    for miss in misses {
        let runtime = runtimes
            .get(&miss.req.cache_root)
            .expect("runtime publication attempted for every miss root");
        let runtime_dir = match runtime {
            PublishedRuntime::Ready(path) => path,
            PublishedRuntime::Error(kind, message) => {
                out[miss.index] = Some(Err(RslipError::Io(io::Error::new(*kind, message.clone()))));
                continue;
            }
        };
        match prepare_rslip_runner_miss(miss, runtime_dir) {
            Ok(runner_miss) => runner_misses.push(runner_miss),
            Err((index, err)) => out[index] = Some(Err(err)),
        }
    }
    runner_misses
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
    miss: RslipCacheCandidate,
    runtime_dir: &Path,
) -> Result<RslipMiss, (usize, RslipError)> {
    fs::create_dir_all(miss.req.cache_root.join("testmon"))
        .map_err(|err| (miss.index, RslipError::Io(err)))?;
    let artifact_path = miss.req.cache_root.join("artifacts").join(format!(
        "{}.{}.json",
        miss.fingerprint,
        rslip_unique_suffix()
    ));
    if let Some(parent) = artifact_path.parent() {
        fs::create_dir_all(parent).map_err(|err| (miss.index, RslipError::Io(err)))?;
    }
    let runner_req = build_pytest_runner_request(&miss.req, runtime_dir, &artifact_path);
    Ok(RslipMiss {
        index: miss.index,
        req: miss.req,
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
        };
        let miss = RslipMiss {
            index: candidate.index,
            req: candidate.req,
            fingerprint: candidate.fingerprint,
            runner_req,
        };

        assert_eq!(miss.index, 3);
        assert_eq!(miss.req.nodeid, req.nodeid);
        assert_eq!(miss.fingerprint, "abc");
        assert_eq!(miss.runner_req.artifacts[0].name, "coverage");
    }
}
