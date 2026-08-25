use std::collections::BTreeSet;

use crate::rslip::{Rslip, RslipError, RslipOutcome};

use super::finalize::{clone_rslip_result, handle_rslip_miss_result};
use super::lock_chunk::{brief_lock_filter_rslip_miss_groups, coalesce_rslip_miss_candidates};
use super::pycache::purge_pycache_under;
use super::{
    PreparedRslipMisses, RslipBatchProgress, RslipCacheCandidate, RslipMiss, prepare_rslip_misses,
};

fn emit_miss_progress(
    outcomes: Vec<(usize, Result<RslipOutcome, RslipError>)>,
    remaining: usize,
    on_progress: &mut impl FnMut(RslipBatchProgress),
) {
    on_progress(RslipBatchProgress::SelectorFinalized { outcomes });
    if remaining == 0 || remaining.is_multiple_of(25) {
        on_progress(RslipBatchProgress::TestsRemaining { remaining });
    }
}

pub(super) fn run_rslip_misses(
    rslip: &Rslip,
    misses: Vec<RslipCacheCandidate>,
    mut remaining: usize,
    jobs: usize,
    out: &mut [Option<Result<RslipOutcome, RslipError>>],
    on_progress: &mut impl FnMut(RslipBatchProgress),
) {
    let groups = coalesce_rslip_miss_candidates(misses);
    let mut seen = slot_filled_mask(out);
    let groups = brief_lock_filter_rslip_miss_groups(groups, out);
    remaining = emit_new_miss_finalizations(out, &mut seen, remaining, on_progress);

    let PreparedRslipMisses {
        misses: runner_misses,
    } = prepare_rslip_misses(groups, out);
    remaining = emit_new_miss_finalizations(out, &mut seen, remaining, on_progress);

    if runner_misses.is_empty() {
        return;
    }

    let source_roots: BTreeSet<_> = runner_misses
        .iter()
        .map(|miss| miss.req.source_root.clone())
        .collect();
    for root in source_roots {
        purge_pycache_under(&root);
    }
    let runner_reqs: Vec<_> = runner_misses
        .iter()
        .map(|miss| miss.runner_req.clone())
        .collect();
    let mut pending: Vec<Option<RslipMiss>> = runner_misses.into_iter().map(Some).collect();
    rslip
        .runner
        .run_many_bounded_with_on_complete(runner_reqs, jobs, &mut |index, result| {
            let miss = pending[index]
                .take()
                .expect("each runner request completes at most once");
            let resolved = miss.indices.len();
            let mut outcomes = Vec::with_capacity(resolved);
            for (slot_index, slot_result) in handle_rslip_miss_result(miss, result) {
                out[slot_index] = Some(clone_rslip_result(&slot_result));
                seen[slot_index] = true;
                outcomes.push((slot_index, slot_result));
            }
            remaining = remaining.saturating_sub(resolved);
            emit_miss_progress(outcomes, remaining, on_progress);
        });
}

fn slot_filled_mask(out: &[Option<Result<RslipOutcome, RslipError>>]) -> Vec<bool> {
    out.iter().map(Option::is_some).collect()
}

fn emit_new_miss_finalizations(
    out: &[Option<Result<RslipOutcome, RslipError>>],
    seen: &mut [bool],
    mut remaining: usize,
    on_progress: &mut impl FnMut(RslipBatchProgress),
) -> usize {
    let mut outcomes = Vec::new();
    for (index, slot) in out.iter().enumerate() {
        if seen[index] {
            continue;
        }
        let Some(result) = slot else {
            continue;
        };
        seen[index] = true;
        outcomes.push((index, clone_rslip_result(result)));
    }
    if outcomes.is_empty() {
        return remaining;
    }
    remaining = remaining.saturating_sub(outcomes.len());
    emit_miss_progress(outcomes, remaining, on_progress);
    remaining
}
