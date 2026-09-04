use super::batch_executor_prepare::{
    PreparedRustBatch, merge_prepared, outcome_from_entry, prepare_rust_batch,
};
use crate::rust_llvm_cov_runner::RustLlvmCovError;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_executor_fresh::execute_fresh_batch_with_exporter;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_export::SubprocessInstanceExporter;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_export_tools::resolve_export_tools_from_rustc;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_lock::try_lock_batch;
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_result::{
    RustCoverageBatchCounters, RustCoverageBatchResult,
};
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_run::default_batch_subprocess_runner;
use crate::rust_llvm_cov_runner::execute_or_reuse::worker::cleanup_legacy_worker_data_nonblocking;
use crate::rust_llvm_cov_runner::file_lock::FileLockGuard;
use crate::rust_llvm_cov_runner::plan::batch_fingerprint::{
    RustCoverageBatchIdentity, RustCoverageToolIdentity,
};
use crate::rust_llvm_cov_runner::plan::batch_plan::{
    CoverageOutputMode, RustCoverageBatchPlan, RustCoverageBatchRequest,
    build_rust_coverage_batch_plan,
};
use crate::rust_llvm_cov_runner::publish_derived::batch_derived::population_derived_state_stale;

fn default_instance_exporter(
    req: &RustCoverageBatchRequest,
    plan: &RustCoverageBatchPlan,
) -> Result<SubprocessInstanceExporter, RustLlvmCovError> {
    let export_tools = resolve_export_tools_from_rustc(std::ffi::OsStr::new("rustc"))?;
    let ignore_filename_regex =
        crate::rust_llvm_cov_runner::execute_or_reuse::batch_export_ignore::resolve_ignore_filename_regex(
            req,
            &plan.build_target,
        )?;
    Ok(SubprocessInstanceExporter::new(
        export_tools,
        ignore_filename_regex,
    ))
}

thread_local! {
    static HELD_LOCK: std::cell::RefCell<Option<FileLockGuard>> =
        const { std::cell::RefCell::new(None) };
    static LOCK_BATCH_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) fn install_held_batch_lock(guard: FileLockGuard) {
    HELD_LOCK.with(|slot| *slot.borrow_mut() = Some(guard));
}

fn lock_batch_with_progress(
    cache_root: &std::path::Path,
) -> std::io::Result<(FileLockGuard, bool)> {
    if let Some(guard) = try_lock_batch(cache_root)? {
        return Ok((guard, false));
    }
    super::progress::log_named_step("batch-lock-wait", || {
        super::batch_lock::wait_for_batch_lock(cache_root)
    })
    .map(|guard| (guard, true))
}

pub fn lock_and_hold_batch(cache_root: &std::path::Path) -> std::io::Result<()> {
    let (guard, _) = lock_batch_with_progress(cache_root)?;
    install_held_batch_lock(guard);
    Ok(())
}

pub fn try_lock_and_hold_batch(cache_root: &std::path::Path) -> std::io::Result<bool> {
    match super::batch_lock::try_lock_batch(cache_root)? {
        Some(guard) => {
            install_held_batch_lock(guard);
            Ok(true)
        }
        None => Ok(false),
    }
}

#[cfg(test)]
pub fn lock_batch_call_count() -> usize {
    LOCK_BATCH_COUNT.with(std::cell::Cell::get)
}

#[cfg(test)]
pub fn reset_lock_batch_call_count() {
    LOCK_BATCH_COUNT.with(|c| c.set(0));
}

pub fn execute_rust_coverage_batch(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
) -> Result<RustCoverageBatchResult, RustLlvmCovError> {
    execute_rust_coverage_batch_with_held_lock(req, tools, None)
}

pub(crate) fn execute_rust_coverage_batch_with_held_lock(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    held_lock: Option<FileLockGuard>,
) -> Result<RustCoverageBatchResult, RustLlvmCovError> {
    if let Some(guard) = held_lock {
        install_held_batch_lock(guard);
    }
    execute_rust_coverage_batch_with_fresh(req, tools, |req, tools, identity, plan| {
        let exporter = crate::rust_llvm_cov_runner::execute_or_reuse::progress::log_named_step(
            "export-prep",
            || default_instance_exporter(req, plan),
        )?;
        execute_fresh_batch_with_exporter(
            req,
            tools,
            identity,
            plan,
            &default_batch_subprocess_runner(),
            exporter,
        )
    })
}

pub(crate) fn execute_rust_coverage_batch_with_fresh<F>(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    fresh: F,
) -> Result<RustCoverageBatchResult, RustLlvmCovError>
where
    F: FnOnce(
        &RustCoverageBatchRequest,
        &RustCoverageToolIdentity,
        &RustCoverageBatchIdentity,
        &RustCoverageBatchPlan,
    ) -> Result<RustCoverageBatchResult, RustLlvmCovError>,
{
    crate::rust_llvm_cov_runner::plan::batch_platform::ensure_batch_platform_supported()?;
    let identity = resolve_batch_identity(req, tools)?;
    if let Some(result) = banned_timeout_batch_result(req) {
        return Ok(result);
    }
    if let Some(result) = try_reuse_before_lock(req, tools, &identity)? {
        return Ok(result);
    }

    let held = HELD_LOCK.with(|slot| slot.borrow_mut().take());
    let (_batch_guard, _) = if let Some(guard) = held {
        (guard, false)
    } else {
        LOCK_BATCH_COUNT.with(|c| c.set(c.get() + 1));
        lock_batch_with_progress(&req.cache_root)?
    };
    let mut prepared = None;
    if matches!(
        req.coverage_output_mode,
        CoverageOutputMode::SelectorEntries
    ) && !req.force_rerun
    {
        match try_all_hit_fast_path(req, tools, &identity)? {
            FastPathProbe::Hit(result) => {
                return Ok(with_process_reverse_query_counters(*result));
            }
            FastPathProbe::Miss(probed) => prepared = probed,
        }
    }
    let legacy_cleanup = cleanup_legacy_worker_data_nonblocking(&req.cache_root)?;

    if matches!(
        req.coverage_output_mode,
        CoverageOutputMode::SelectorEntries
    ) && !req.force_rerun
    {
        return run_locked_selector_entries(
            req,
            tools,
            &identity,
            fresh,
            legacy_cleanup.deferred,
            prepared,
        );
    }

    let plan = crate::rust_llvm_cov_runner::execute_or_reuse::progress::log_named_step(
        "batch-plan",
        || {
            build_rust_coverage_batch_plan(req).map_err(|message| {
                RustLlvmCovError::InvalidRequest(format!("batch plan: {message}"))
            })
        },
    )?;
    if let Some(mut result) = try_check_aggregate_hit(req, &identity)? {
        result.counters.legacy_cleanup_deferred = legacy_cleanup.deferred;
        return Ok(with_process_reverse_query_counters(result));
    }

    fresh(req, tools, &identity, &plan).and_then(|mut result| {
        finalize_after_fresh_batch(req, tools, &identity, legacy_cleanup.deferred, &mut result)?;
        Ok(with_process_reverse_query_counters(result))
    })
}

#[path = "batch_executor_reuse.rs"]
mod reuse;
use reuse::{
    banned_timeout_batch_result, resolve_batch_identity, try_check_aggregate_hit,
    try_reuse_before_lock,
};

fn run_locked_selector_entries<F>(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    fresh: F,
    deferred: bool,
    prepared: Option<PreparedRustBatch>,
) -> Result<RustCoverageBatchResult, RustLlvmCovError>
where
    F: FnOnce(
        &RustCoverageBatchRequest,
        &RustCoverageToolIdentity,
        &RustCoverageBatchIdentity,
        &RustCoverageBatchPlan,
    ) -> Result<RustCoverageBatchResult, RustLlvmCovError>,
{
    let prepared = match prepared {
        Some(prepared) => prepared,
        None => prepare_rust_batch(req, tools, identity)?,
    };
    if prepared.misses_empty() {
        return maybe_publish_derived_after_all_hit(
            req,
            tools,
            identity,
            prepared.hit_result(&req.logical_selectors),
        )
        .map(|mut result| {
            result.counters.legacy_cleanup_deferred = deferred;
            with_process_reverse_query_counters(result)
        });
    }
    let mut miss_req = req.clone();
    miss_req.logical_selectors = prepared.misses.clone();
    let miss_plan = crate::rust_llvm_cov_runner::execute_or_reuse::progress::log_named_step(
        "batch-plan",
        || {
            build_rust_coverage_batch_plan(&miss_req).map_err(|message| {
                RustLlvmCovError::InvalidRequest(format!("batch plan: {message}"))
            })
        },
    )?;
    fresh(&miss_req, tools, identity, &miss_plan).and_then(|fresh_result| {
        let mut result = merge_prepared(&prepared, &req.logical_selectors, Some(fresh_result));
        finalize_after_fresh_batch(req, tools, identity, deferred, &mut result)?;
        Ok(with_process_reverse_query_counters(result))
    })
}

pub(super) fn with_process_reverse_query_counters(
    mut result: RustCoverageBatchResult,
) -> RustCoverageBatchResult {
    result.counters.incorporate_process_reverse_query_counters();
    result
}

fn finalize_after_fresh_batch(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    legacy_cleanup_deferred: bool,
    result: &mut RustCoverageBatchResult,
) -> Result<(), RustLlvmCovError> {
    if !matches!(
        req.coverage_output_mode,
        CoverageOutputMode::SelectorEntries
    ) {
        result.counters.legacy_cleanup_deferred = legacy_cleanup_deferred;
        return Ok(());
    }
    apply_population_derived_publication(req, tools, identity, result)?;
    crate::rust_llvm_cov_runner::publish_derived::batch_derived::maybe_prune_obsolete_selective_after_batch(
        req, identity, result,
    )?;
    super::batch_executor_sealed::write_seal_after_complete_pass(req, identity, result);
    result.counters.legacy_cleanup_deferred = legacy_cleanup_deferred;
    Ok(())
}

fn maybe_publish_derived_after_all_hit(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    mut result: RustCoverageBatchResult,
) -> Result<RustCoverageBatchResult, RustLlvmCovError> {
    apply_population_derived_publication(req, tools, identity, &mut result)?;
    super::batch_executor_sealed::write_seal_after_complete_pass(req, identity, &result);
    Ok(result)
}

fn apply_population_derived_publication(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
    result: &mut RustCoverageBatchResult,
) -> Result<(), RustLlvmCovError> {
    if result.batch_error.is_some() {
        return Ok(());
    }
    let selectors = req
        .population_publication_selectors
        .as_deref()
        .unwrap_or(&[]);
    if let Some(publish) =
        crate::rust_llvm_cov_runner::publish_derived::batch_derived::try_publish_population_derived_state_with_binaries(
            req,
            tools,
            identity,
            selectors,
            &result.test_binaries,
        )?
    {
        result.counters.derived_state_published = true;
        result.counters.derived_repair = publish.derived_repair;
        result.counters.entry_generation_count = publish.entry_generation_count;
        result.counters.current_index_generation = publish.current_index_generation;
        result.counters.cache_pruned_entries = publish.cache_pruned_entries;
        result.counters.reverse_published = publish.reverse_published;
        result.counters.reverse_snapshots_reclaimed = publish.reverse_snapshots_reclaimed;
    }
    Ok(())
}

enum FastPathProbe {
    Hit(Box<RustCoverageBatchResult>),
    Miss(Option<PreparedRustBatch>),
}

fn try_all_hit_fast_path(
    req: &RustCoverageBatchRequest,
    tools: &RustCoverageToolIdentity,
    identity: &RustCoverageBatchIdentity,
) -> Result<FastPathProbe, RustLlvmCovError> {
    if let Some(result) = super::batch_executor_sealed::try_sealed_all_hit(req, identity, tools) {
        return Ok(FastPathProbe::Hit(Box::new(result)));
    }
    if population_derived_state_stale(req, tools, identity)? {
        return Ok(FastPathProbe::Miss(None));
    }
    let prepared = prepare_rust_batch(req, tools, identity)?;
    if !prepared.misses_empty() {
        return Ok(FastPathProbe::Miss(Some(prepared)));
    }
    Ok(FastPathProbe::Hit(Box::new(
        prepared.hit_result(&req.logical_selectors),
    )))
}

#[cfg(test)]
#[path = "batch_executor_test.rs"]
mod tests;

#[cfg(test)]
#[path = "batch_executor_b_test.rs"]
mod tests_b;

#[cfg(test)]
#[path = "batch_executor_all_hit_lock_test.rs"]
mod all_hit_lock_tests;

#[cfg(test)]
#[path = "batch_executor_partition_test.rs"]
mod partition_tests;

#[cfg(test)]
mod held_lock_test {
    use super::{lock_batch_call_count, reset_lock_batch_call_count};
    use crate::rust_llvm_cov_runner::execute_or_reuse::batch_lock::{lock_batch, try_lock_batch};

    #[test]
    fn preheld_lock_skips_nested_lock_and_try_lock_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        reset_lock_batch_call_count();
        let guard = lock_batch(tmp.path()).unwrap();
        assert!(try_lock_batch(tmp.path()).unwrap().is_none());
        super::install_held_batch_lock(guard);
        assert_eq!(lock_batch_call_count(), 0);
        drop(super::HELD_LOCK.with(|slot| slot.borrow_mut().take()));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if try_lock_batch(tmp.path()).unwrap().is_some() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "batch lock remained held after dropping its guard"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}
