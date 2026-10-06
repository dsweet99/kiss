use crate::rslip::cache::{
    RslipCacheEntry, load_reusable_rslip_cache_entry, load_rslip_cache_entry,
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
    reqs.iter()
        .map(|req| load_one(req, validate_reuse))
        .collect()
}

fn load_one(req: &RslipRequest, validate_reuse: bool) -> Result<Option<RslipOutcome>, RslipError> {
    validate_rslip_request(req)?;
    let entry = if validate_reuse {
        load_reusable_rslip_cache_entry(req)
    } else {
        load_rslip_cache_entry(req).filter(|entry| !entry.coverage.files.is_empty())
    };
    Ok(entry.map(rslip_outcome_from_cache))
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
