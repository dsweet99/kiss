use std::io;

use crate::rslip::cache::{
    RslipCacheEntry, load_reusable_rslip_cache_entry, load_rslip_cache_entry,
    rslip_cache_fingerprint_from_context, rslip_request_context_fingerprint,
};
use crate::rslip::{CacheStatus, RslipError, RslipOutcome, RslipRequest, validate_rslip_request};

pub fn load_cached_outcomes_many(
    reqs: &[RslipRequest],
) -> Vec<Result<Option<RslipOutcome>, RslipError>> {
    load_cached_outcomes_many_with_reuse(reqs, true)
}

pub fn load_cached_outcomes_many_trusting_population(
    reqs: &[RslipRequest],
) -> Vec<Result<Option<RslipOutcome>, RslipError>> {
    load_cached_outcomes_many_with_reuse(reqs, false)
}

fn load_cached_outcomes_many_with_reuse(
    reqs: &[RslipRequest],
    validate_reuse: bool,
) -> Vec<Result<Option<RslipOutcome>, RslipError>> {
    let Some(first) = reqs.first() else {
        return Vec::new();
    };
    let shared_context = match rslip_request_context_fingerprint(first) {
        Ok(context) => context,
        Err(err) => {
            return reqs
                .iter()
                .map(|_| Err(RslipError::Io(io::Error::new(err.kind(), err.to_string()))))
                .collect();
        }
    };
    reqs.iter()
        .map(|req| load_one(req, first, &shared_context, validate_reuse))
        .collect()
}

fn load_one(
    req: &RslipRequest,
    first: &RslipRequest,
    shared_context: &str,
    validate_reuse: bool,
) -> Result<Option<RslipOutcome>, RslipError> {
    validate_rslip_request(req)?;
    let fingerprint = if rslip_requests_share_context(first, req) {
        rslip_cache_fingerprint_from_context(shared_context, &req.nodeid)
    } else {
        let context = rslip_request_context_fingerprint(req)?;
        rslip_cache_fingerprint_from_context(&context, &req.nodeid)
    };
    let entry = if validate_reuse {
        load_reusable_rslip_cache_entry(&req.cache_root, &fingerprint, &req.source_root)
    } else {
        load_rslip_cache_entry(&req.cache_root, &fingerprint)
            .filter(|entry| !entry.coverage.files.is_empty())
    };
    Ok(entry.map(rslip_outcome_from_cache))
}

fn rslip_requests_share_context(first: &RslipRequest, other: &RslipRequest) -> bool {
    first.cwd == other.cwd
        && first.source_root == other.source_root
        && first.python == other.python
        && first.python_version == other.python_version
        && first.pytest_version == other.pytest_version
        && first.pytest_args == other.pytest_args
        && first.env == other.env
        && first.cache_root == other.cache_root
}

pub(crate) fn rslip_outcome_from_cache(entry: RslipCacheEntry) -> RslipOutcome {
    RslipOutcome {
        nodeid: entry.nodeid,
        status: entry.status,
        exit_code: entry.exit_code,
        duration: entry.duration,
        coverage: entry.coverage,
        cache_status: CacheStatus::Hit,
        stdout: None,
        stderr: None,
    }
}
