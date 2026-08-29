use std::collections::{BTreeMap, BTreeSet};

use super::{FreshBatchFinishContext, FreshCheckAggregateExport};
use crate::rust_llvm_cov_runner::{
    RustLineCoverage, RustLlvmCovError, RustLlvmCovOutcome,
    batch_check_aggregate::{
        build_check_aggregate, publish_check_aggregate, selector_binary_ids_from_outcomes,
    },
    batch_executor_finish_store::store_completed_outcomes,
    batch_fingerprint::{RustCoverageBatchIdentity, RustCoverageToolIdentity},
    batch_plan::RustCoverageBatchRequest,
    batch_result::{RustCoverageBatchCounters, RustCoverageBatchResult},
};

pub(super) fn store_and_publish_check_aggregate(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    export: FreshCheckAggregateExport,
    finish: FreshBatchFinishContext,
    mut completed: Vec<RustLlvmCovOutcome>,
    mut counters: RustCoverageBatchCounters,
) -> Result<RustCoverageBatchResult, RustLlvmCovError> {
    let (aggregate_selectors, selector_binary_ids, test_binaries, binary_line_maps) =
        match finish.repair_publication.clone() {
            Some(repair) => {
                let selectors = repair
                    .selector_binary_ids
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>();
                let mut maps = repair.retained_binary_line_maps;
                maps.extend(export.exported);
                (
                    selectors,
                    repair.selector_binary_ids,
                    repair.test_binaries,
                    maps,
                )
            }
            None => (
                req.logical_selectors.clone(),
                selector_binary_ids_from_outcomes(&completed),
                finish.test_binaries.clone(),
                export.exported,
            ),
        };

    for outcome in &mut completed {
        outcome.coverage = RustLineCoverage {
            files: BTreeMap::new(),
        };
    }
    if let Err(store_err) = crate::rust_llvm_cov_runner::execute_or_reuse::progress::log_named_step(
        "entry-store",
        || store_completed_outcomes(req, tools, identity, &mut completed),
    ) {
        return Ok(RustCoverageBatchResult {
            completed,
            batch_error: Some(store_err),
            counters,
            test_binaries: Vec::new(),
        });
    }
    if let Some(repair) = &finish.repair_publication {
        let fresh: BTreeSet<_> = completed.iter().map(|o| o.selector.clone()).collect();
        let retained: Vec<String> = aggregate_selectors
            .iter()
            .filter(|selector| !fresh.contains(*selector))
            .cloned()
            .collect();
        if let Err(store_err) = crate::rust_llvm_cov_runner::rekey_selector_entries_to_identity(
            req,
            tools,
            identity,
            &repair.prior_generation,
            &retained,
        ) {
            return Ok(RustCoverageBatchResult {
                completed,
                batch_error: Some(store_err),
                counters,
                test_binaries: Vec::new(),
            });
        }
    }
    let aggregate = build_check_aggregate(
        req,
        identity,
        &aggregate_selectors,
        selector_binary_ids,
        &test_binaries,
        binary_line_maps,
    )?;
    crate::rust_llvm_cov_runner::execute_or_reuse::progress::log_named_step(
        "derived-publish",
        || {
            publish_check_aggregate(req, &aggregate)?;
            crate::rust_llvm_cov_runner::publish_derived::batch_derived::publish_conservative_derived_state_from_check_aggregate(
            req, tools, identity, &aggregate,
        )?;
            Ok::<(), RustLlvmCovError>(())
        },
    )?;
    counters.aggregate_binaries = aggregate.binaries.len();
    Ok(RustCoverageBatchResult {
        completed,
        batch_error: None,
        counters,
        test_binaries: finish.test_binaries,
    })
}
