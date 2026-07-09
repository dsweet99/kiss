use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rpytest_runner::TestStatus;

use crate::llvm_cov_json::parse_llvm_cov_json_file;
use crate::rust_cov_cache::{
    RustCovCacheEntry, rust_cov_unique_suffix, store_rust_cov_cache_entry,
};
use crate::worker::cleanup_worker_slot_transients;
use crate::{
    CargoLlvmCovRunOutcome, RustCovCacheStatus, RustLineCoverage, RustLlvmCovError,
    RustLlvmCovOutcome, RustLlvmCovRequest,
};

pub(crate) fn rust_cov_artifact_path(cache_root: &Path, fingerprint: &str) -> PathBuf {
    cache_root
        .join("artifacts")
        .join(format!("{fingerprint}.{}.json", rust_cov_unique_suffix()))
}

pub(crate) fn finalize_run(
    req: &RustLlvmCovRequest,
    fingerprint: &str,
    run: Result<CargoLlvmCovRunOutcome, RustLlvmCovError>,
) -> Result<RustLlvmCovOutcome, RustLlvmCovError> {
    match run {
        Ok(run) if run.status == TestStatus::Passed => finalize_passed_run(req, fingerprint, run),
        Ok(run) => finalize_nonzero_run(req, fingerprint, run),
        Err(err) => {
            let cleanup = collect_cleanup_errors(req);
            Err(combine_primary_and_finalization(err, cleanup))
        }
    }
}

fn finalize_passed_run(
    req: &RustLlvmCovRequest,
    fingerprint: &str,
    run: CargoLlvmCovRunOutcome,
) -> Result<RustLlvmCovOutcome, RustLlvmCovError> {
    let coverage = match parse_llvm_cov_json_file(&run.artifact_path, &req.source_root) {
        Ok(coverage) => coverage,
        Err(err) => {
            let cleanup = collect_cleanup_errors(req);
            return Err(combine_primary_and_finalization(err, cleanup));
        }
    };
    let outcome = RustLlvmCovOutcome {
        selector: run.selector,
        status: run.status,
        exit_code: run.exit_code,
        duration: run.duration,
        coverage,
        cache_status: RustCovCacheStatus::MissStored,
        stdout: Some(run.stdout),
        stderr: Some(run.stderr),
    };
    let mut finalization = Vec::new();
    if let Err(err) = store_rust_cov_cache_entry(
        &req.cache_root,
        fingerprint,
        &RustCovCacheEntry::from(&outcome),
    ) {
        finalization.push(RustLlvmCovError::Io(err));
    } else if let Err(err) = fs::remove_file(&run.artifact_path) {
        finalization.push(RustLlvmCovError::Io(err));
    }
    finalization.extend(collect_cleanup_errors(req));
    if finalization.is_empty() {
        Ok(outcome)
    } else {
        Err(RustLlvmCovError::Composite {
            primary: Box::new(RustLlvmCovError::InvalidRequest(
                "rust llvm-cov finalization failed after successful execution".to_string(),
            )),
            finalization,
        })
    }
}

fn finalize_nonzero_run(
    req: &RustLlvmCovRequest,
    fingerprint: &str,
    run: CargoLlvmCovRunOutcome,
) -> Result<RustLlvmCovOutcome, RustLlvmCovError> {
    let outcome = RustLlvmCovOutcome {
        selector: run.selector,
        status: run.status,
        exit_code: run.exit_code,
        duration: run.duration,
        coverage: RustLineCoverage {
            files: BTreeMap::new(),
        },
        cache_status: RustCovCacheStatus::MissStored,
        stdout: Some(run.stdout),
        stderr: Some(run.stderr),
    };
    let mut finalization = Vec::new();
    if let Err(err) = store_rust_cov_cache_entry(
        &req.cache_root,
        fingerprint,
        &RustCovCacheEntry::from(&outcome),
    ) {
        finalization.push(RustLlvmCovError::Io(err));
    }
    match fs::remove_file(&run.artifact_path) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => finalization.push(RustLlvmCovError::Io(err)),
    }
    finalization.extend(collect_cleanup_errors(req));
    if finalization.is_empty() {
        Ok(outcome)
    } else {
        Err(RustLlvmCovError::Composite {
            primary: Box::new(RustLlvmCovError::InvalidRequest(
                "rust llvm-cov finalization failed after nonzero test execution".to_string(),
            )),
            finalization,
        })
    }
}

fn collect_cleanup_errors(req: &RustLlvmCovRequest) -> Vec<RustLlvmCovError> {
    cleanup_worker_slot_transients(&req.cache_root, req.worker_slot)
        .err()
        .map(RustLlvmCovError::Io)
        .into_iter()
        .collect()
}

pub(crate) fn combine_primary_and_finalization(
    primary: RustLlvmCovError,
    finalization: Vec<RustLlvmCovError>,
) -> RustLlvmCovError {
    if finalization.is_empty() {
        primary
    } else {
        RustLlvmCovError::Composite {
            primary: Box::new(primary),
            finalization,
        }
    }
}
