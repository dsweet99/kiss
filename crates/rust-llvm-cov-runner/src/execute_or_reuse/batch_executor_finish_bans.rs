use std::collections::BTreeMap;
use std::time::Duration;

use crate::{
    RustCovCacheStatus, RustLineCoverage, RustLlvmCovOutcome,
    batch_aggregate::{InstanceResult, aggregate_logical_selectors},
    batch_plan::RustCoverageBatchRequest,
};

pub(crate) fn aggregate_with_zero_limit_bans(
    req: &RustCoverageBatchRequest,
    exact: bool,
    instances: &[InstanceResult],
) -> (Vec<RustLlvmCovOutcome>, usize) {
    let runnable: Vec<String> = req
        .logical_selectors
        .iter()
        .filter(|selector| !selector_timeout_is_ban(req, selector))
        .cloned()
        .collect();
    let (runnable_outcomes, counters) = aggregate_logical_selectors(&runnable, exact, instances);
    let mut by_selector: BTreeMap<String, RustLlvmCovOutcome> = runnable_outcomes
        .into_iter()
        .map(|outcome| (outcome.selector.clone(), outcome))
        .collect();
    let mut completed = Vec::with_capacity(req.logical_selectors.len());
    for selector in &req.logical_selectors {
        if selector_timeout_is_ban(req, selector) {
            completed.push(banned_timeout_outcome(selector));
            continue;
        }
        if let Some(outcome) = by_selector.remove(selector) {
            completed.push(outcome);
        }
    }
    (completed, counters.unmatched_selectors)
}

fn selector_timeout_is_ban(req: &RustCoverageBatchRequest, selector: &str) -> bool {
    req.selector_timeout_millis.get(selector) == Some(&0)
}

fn banned_timeout_outcome(selector: &str) -> RustLlvmCovOutcome {
    RustLlvmCovOutcome {
        selector: selector.to_string(),
        status: rpytest_runner::TestStatus::TimedOut,
        exit_code: Some(124),
        duration: Duration::ZERO,
        coverage: RustLineCoverage {
            files: BTreeMap::new(),
        },
        test_binary_ids: Vec::new(),
        cache_status: RustCovCacheStatus::FreshUnstored,
        stdout: None,
        stderr: None,
    }
}

pub(crate) fn unmatched_selectors_batch_error(
    kind: &str,
    unmatched_selectors: usize,
    counters: crate::batch_result::RustCoverageBatchCounters,
) -> Option<crate::batch_result::RustCoverageBatchResult> {
    (unmatched_selectors > 0).then(|| crate::batch_result::RustCoverageBatchResult {
        completed: Vec::new(),
        batch_error: Some(crate::RustLlvmCovError::InvalidRequest(format!(
            "{kind} batch did not execute {unmatched_selectors} requested Rust selector(s)"
        ))),
        counters,
        test_binaries: Vec::new(),
    })
}
