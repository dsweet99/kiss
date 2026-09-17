use std::fs;
use std::io;
use std::path::PathBuf;

use crate::rpytest_runner::{PytestRunError, PytestRunOutcome, PytestRunRequest, TestStatus};

use crate::rslip::cache::{
    DigestMemo, RslipCacheEntry, load_reusable_rslip_cache_entry, store_rslip_cache_entry,
};
use crate::rslip::{
    CacheStatus, RslipError, RslipOutcome, rslip_coverage_from_outcome, rslip_outcome_from_cache,
};

use super::RslipMiss;

pub(super) fn handle_rslip_miss_result(
    miss: RslipMiss,
    result: Result<PytestRunOutcome, PytestRunError>,
    memo: &mut DigestMemo,
) -> Vec<(usize, Result<RslipOutcome, RslipError>)> {
    let result = handle_rslip_miss_result_once(&miss, result, memo);
    miss.indices
        .into_iter()
        .map(|index| (index, clone_rslip_result(&result)))
        .collect()
}

struct RemoveArtifactsOnDrop {
    paths: Vec<PathBuf>,
}

impl Drop for RemoveArtifactsOnDrop {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = fs::remove_file(path);
        }
    }
}

fn artifact_cleanup(req: &PytestRunRequest) -> RemoveArtifactsOnDrop {
    let mut paths: Vec<PathBuf> = req
        .artifacts
        .iter()
        .map(|artifact| artifact.path.clone())
        .collect();
    if let Some(testmon) = req.env.get("TESTMON_DATAFILE") {
        paths.push(PathBuf::from(testmon));
    }
    RemoveArtifactsOnDrop { paths }
}

fn handle_rslip_miss_result_once(
    miss: &RslipMiss,
    result: Result<PytestRunOutcome, PytestRunError>,
    memo: &mut DigestMemo,
) -> Result<RslipOutcome, RslipError> {
    let _cleanup = artifact_cleanup(&miss.runner_req);
    let outcome = match result {
        Ok(outcome) => outcome,

        Err(PytestRunError::Timeout(timeout)) => {
            return finalize_timed_out_miss_outcome(miss, timeout, 124, String::new());
        }
        Err(err) => return Err(RslipError::Runner(err)),
    };
    let coverage = match rslip_coverage_from_outcome(&outcome) {
        Ok(coverage) => coverage,

        Err(RslipError::MissingArtifact(name)) => {
            return finalize_failed_miss_outcome(
                miss,
                outcome.duration,
                outcome.exit_code.unwrap_or(1),
                missing_artifact_stderr(&outcome.stderr, &name),
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
    finalize_cacheable_miss_outcome(miss, rslip_outcome, memo)
}

fn missing_artifact_stderr(child_stderr: &[u8], name: &str) -> String {
    let mut combined = String::from_utf8_lossy(child_stderr).into_owned();
    if !combined.is_empty() && !combined.ends_with('\n') {
        combined.push('\n');
    }
    combined.push_str(&format!("rslip: missing coverage artifact {name}\n"));
    combined
}

fn finalize_failed_miss_outcome(
    miss: &RslipMiss,
    duration: std::time::Duration,
    exit_code: i32,
    stderr: String,
) -> Result<RslipOutcome, RslipError> {
    finalize_status_miss_outcome(miss, TestStatus::Failed, duration, exit_code, stderr)
}

fn finalize_timed_out_miss_outcome(
    miss: &RslipMiss,
    duration: std::time::Duration,
    exit_code: i32,
    stderr: String,
) -> Result<RslipOutcome, RslipError> {
    finalize_status_miss_outcome(miss, TestStatus::TimedOut, duration, exit_code, stderr)
}

fn finalize_status_miss_outcome(
    miss: &RslipMiss,
    status: TestStatus,
    duration: std::time::Duration,
    exit_code: i32,
    stderr: String,
) -> Result<RslipOutcome, RslipError> {
    Ok(RslipOutcome {
        nodeid: miss.req.nodeid.clone(),
        status,
        exit_code: Some(exit_code),
        duration,
        coverage: crate::rslip::LineCoverage {
            files: std::collections::BTreeMap::new(),
        },
        cache_status: CacheStatus::MissStored,
        stdout: None,
        stderr: Some(stderr.into_bytes()),
    })
}

fn finalize_cacheable_miss_outcome(
    miss: &RslipMiss,
    outcome: RslipOutcome,
    memo: &mut DigestMemo,
) -> Result<RslipOutcome, RslipError> {
    if outcome.status != TestStatus::Passed || outcome.coverage.files.is_empty() {
        return Ok(outcome);
    }
    let _guard = crate::rslip::lock_rslip_cache_entry(&miss.req.cache_root, &miss.fingerprint)?;
    if !miss.req.force_rerun
        && let Some(entry) = load_reusable_rslip_cache_entry(
            &miss.req.cache_root,
            &miss.fingerprint,
            &miss.req.source_root,
        )
    {
        return Ok(rslip_outcome_from_cache(entry));
    }
    store_rslip_cache_entry(
        &miss.req.cache_root,
        &miss.fingerprint,
        &RslipCacheEntry::from_outcome_with_memo(&outcome, &miss.req.source_root, memo),
    )?;
    Ok(outcome)
}

pub(super) fn clone_rslip_result(
    result: &Result<RslipOutcome, RslipError>,
) -> Result<RslipOutcome, RslipError> {
    match result {
        Ok(outcome) => Ok(outcome.clone()),
        Err(err) => Err(clone_rslip_error(err)),
    }
}

pub(super) fn clone_rslip_error(err: &RslipError) -> RslipError {
    match err {
        RslipError::Io(err) => RslipError::Io(io::Error::new(err.kind(), err.to_string())),
        RslipError::Json(err) => {
            RslipError::Json(serde_json::Error::io(io::Error::other(err.to_string())))
        }
        RslipError::Runner(err) => RslipError::Runner(err.cloned()),
        RslipError::MissingArtifact(name) => RslipError::MissingArtifact(name.clone()),
        RslipError::InvalidRequest(message) => RslipError::InvalidRequest(message.clone()),
    }
}
