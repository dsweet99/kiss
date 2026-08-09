use std::io;

use rpytest_runner::{PytestRunError, PytestRunOutcome};

use crate::cache::{RslipCacheEntry, load_reusable_rslip_cache_entry, store_rslip_cache_entry};
use crate::{
    CacheStatus, RslipError, RslipOutcome, rslip_coverage_from_outcome, rslip_outcome_from_cache,
};

use super::RslipMiss;

pub(super) fn handle_rslip_miss_result(
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
        // Timeouts become TimedOut outcomes (empty coverage) so a large
        // population can finish instead of hanging forever on one stuck test.
        Err(PytestRunError::Timeout(timeout)) => {
            // TimedOut is already reported on stdout as TIMEOUT; no stderr noise.
            return finalize_timed_out_miss_outcome(miss, timeout, 124, String::new());
        }
        Err(err) => return Err(RslipError::Runner(err)),
    };
    let coverage = match rslip_coverage_from_outcome(&outcome) {
        Ok(coverage) => coverage,
        // Missing coverage must not abort a multi-thousand-selector population.
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
    finalize_cacheable_miss_outcome(miss, rslip_outcome)
}

/// Keep child stderr (e.g. ignored atexit coverage-write failures) and append
/// the synthetic missing-artifact line. Empty child stderr yields only that line.
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
    finalize_status_miss_outcome(
        miss,
        rpytest_runner::TestStatus::Failed,
        duration,
        exit_code,
        stderr,
    )
}

fn finalize_timed_out_miss_outcome(
    miss: &RslipMiss,
    duration: std::time::Duration,
    exit_code: i32,
    stderr: String,
) -> Result<RslipOutcome, RslipError> {
    finalize_status_miss_outcome(
        miss,
        rpytest_runner::TestStatus::TimedOut,
        duration,
        exit_code,
        stderr,
    )
}

fn finalize_status_miss_outcome(
    miss: &RslipMiss,
    status: rpytest_runner::TestStatus,
    duration: std::time::Duration,
    exit_code: i32,
    stderr: String,
) -> Result<RslipOutcome, RslipError> {
    let rslip_outcome = RslipOutcome {
        nodeid: miss.req.nodeid.clone(),
        status,
        exit_code: Some(exit_code),
        duration,
        coverage: crate::LineCoverage {
            files: std::collections::BTreeMap::new(),
        },
        cache_status: CacheStatus::MissStored,
        stdout: None,
        stderr: Some(stderr.into_bytes()),
    };
    finalize_cacheable_miss_outcome(miss, rslip_outcome)
}

/// Lock, recheck for a concurrent cache entry, then store this outcome if still missing.
fn finalize_cacheable_miss_outcome(
    miss: &RslipMiss,
    outcome: RslipOutcome,
) -> Result<RslipOutcome, RslipError> {
    let _guard = crate::lock_rslip_cache_entry(&miss.req.cache_root, &miss.fingerprint)?;
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
        &RslipCacheEntry::from_outcome(&outcome, &miss.req.source_root),
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
