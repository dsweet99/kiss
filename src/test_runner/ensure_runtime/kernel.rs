use std::collections::BTreeSet;

use crate::test_runner::lang_iface::{
    AcceptMode, EnsureRequest, EnsureRuntimeResult, KernelRules, LanguageEnsureResult,
    LanguageRuntime, OutcomeBatch, all_misses_warm_skippable, emit_kernel_stage,
    reclassify_statuses_with_gate, timing_context_is_comparable,
};
use kiss::GateConfig;

pub(crate) fn ensure_runtime_cache(
    request: &EnsureRequest,
    modules: &[&dyn LanguageRuntime],
) -> Result<EnsureRuntimeResult, String> {
    kiss::subprocess_observer::reset_subprocess_observer();
    let mut result = EnsureRuntimeResult::default();
    let gate = &request.gate;
    for module in modules {
        let language = module.language();
        if !request.requires(language) {
            continue;
        }
        let lang_result = ensure_one_language(request, *module, gate)?;
        let exit = lang_result.summary.exit_code;
        *result.by_language.get_mut(language) = Some(lang_result);
        if exit != 0 {
            result.exit_code = exit;
        }
    }
    Ok(result)
}

fn ensure_one_language(
    request: &EnsureRequest,
    module: &dyn LanguageRuntime,
    gate: &GateConfig,
) -> Result<LanguageEnsureResult, String> {
    rules(module).bind_subprocess_observer(request);
    if let Some(empty) = try_publish_empty_all(request, module)? {
        return Ok(empty);
    }
    let listing = timed_list(request, module)?;
    let (mut witness, holding) = match timed_load_witness(request, module, &listing) {
        Some(stored) => (Some(stored.witness), stored.holding),
        None => (None, BTreeSet::new()),
    };
    timed_reclassify_witness(request, module, gate, &mut witness)?;
    let misses = timed_compute_misses(
        request,
        module,
        MissInputs {
            planned: &listing.ids,
            identity: &listing.identity,
            witness: &witness,
            holding: &holding,
        },
    )?;
    timed_accept_or_run(request, module, &listing.ids, witness, &misses)
}

fn timed_list(
    request: &EnsureRequest,
    module: &dyn LanguageRuntime,
) -> Result<crate::test_runner::lang_iface::Listing, String> {
    let started = std::time::Instant::now();
    let listing = module.list(request)?;
    crate::test_runner::emit_stage_time(rules(module).identity_stage(), started.elapsed());
    Ok(listing)
}

fn timed_load_witness(
    request: &EnsureRequest,
    module: &dyn LanguageRuntime,
    listing: &crate::test_runner::lang_iface::Listing,
) -> Option<super::rows::StoredRows> {
    let started = std::time::Instant::now();
    let loaded = super::rows::stored_rows(request, module, listing);
    emit_kernel_stage(rules(module), "witness_load", started);
    loaded
}

fn timed_reclassify_witness(
    request: &EnsureRequest,
    module: &dyn LanguageRuntime,
    gate: &GateConfig,
    witness: &mut Option<crate::test_runner::lang_iface::ExecutionWitness>,
) -> Result<(), String> {
    let started = std::time::Instant::now();
    reclassify_loaded_witness(request, module, gate, witness)?;
    emit_kernel_stage(rules(module), "reclassify", started);
    Ok(())
}

struct MissInputs<'a> {
    planned: &'a [String],
    identity: &'a str,
    witness: &'a Option<crate::test_runner::lang_iface::ExecutionWitness>,
    holding: &'a BTreeSet<String>,
}

fn timed_compute_misses(
    request: &EnsureRequest,
    module: &dyn LanguageRuntime,
    inputs: MissInputs<'_>,
) -> Result<Vec<String>, String> {
    let MissInputs {
        planned,
        identity,
        witness,
        holding,
    } = inputs;
    let started = std::time::Instant::now();
    let mut misses = rules(module).live_misses(request, planned, identity, witness.as_ref());
    let not_holding: Vec<String> = planned
        .iter()
        .filter(|selector| !holding.contains(*selector))
        .cloned()
        .collect();
    crate::test_runner::lang_iface::union_force_selectors_into_misses(
        planned,
        &mut misses,
        &not_holding,
    );
    crate::test_runner::lang_iface::union_force_selectors_into_misses(
        planned,
        &mut misses,
        &request.force_selectors,
    );
    union_non_cacheable_misses(planned, &mut misses);
    union_incomparable_timing_misses(request, module, planned, witness, &mut misses);
    misses = planned.to_vec();
    emit_kernel_stage(rules(module), "miss_select", started);
    Ok(misses)
}

fn timed_accept_or_run(
    request: &EnsureRequest,
    module: &dyn LanguageRuntime,
    planned: &[String],
    witness: Option<crate::test_runner::lang_iface::ExecutionWitness>,
    misses: &[String],
) -> Result<LanguageEnsureResult, String> {
    let started = std::time::Instant::now();
    if let Some(accepted) = try_accept_or_warm_report(request, module, planned, &witness, misses)? {
        emit_kernel_stage(rules(module), "accept", started);
        return Ok(accepted);
    }
    emit_kernel_stage(rules(module), "accept", started);
    run_misses_and_maybe_publish(request, module, planned, witness, misses)
}

fn union_non_cacheable_misses(planned: &[String], misses: &mut Vec<String>) {
    let policy = kiss::TestSectionConfig::load().cache_policy;
    let banned: Vec<String> = planned
        .iter()
        .filter(|sel| policy.is_non_cacheable(sel))
        .cloned()
        .collect();
    crate::test_runner::lang_iface::union_force_selectors_into_misses(planned, misses, &banned);
}

fn reclassify_loaded_witness(
    request: &EnsureRequest,
    module: &dyn LanguageRuntime,
    gate: &GateConfig,
    witness: &mut Option<crate::test_runner::lang_iface::ExecutionWitness>,
) -> Result<(), String> {
    let Some(w) = witness.as_mut() else {
        return Ok(());
    };
    if w.raw_statuses.len() != w.statuses.len() {
        w.raw_statuses = w.statuses.clone();
    }
    if !timing_context_matches(request, module) {
        w.statuses = w.raw_statuses.clone();
        return Ok(());
    }
    let gate_selectors = match rules(module).selectors_for_time_gate(request, &w.selectors) {
        Ok(selectors) => selectors,
        Err(err) if rules(module).time_gate_selector_error_is_fatal() => return Err(err),
        Err(_) => w.selectors.clone(),
    };
    w.statuses =
        reclassify_statuses_with_gate(&gate_selectors, &w.raw_statuses, &w.durations_ns, gate);
    Ok(())
}

fn try_publish_empty_all(
    request: &EnsureRequest,
    module: &dyn LanguageRuntime,
) -> Result<Option<LanguageEnsureResult>, String> {
    if !(request.planned_for(module.language()).is_empty() && request.mode == AcceptMode::All) {
        return Ok(None);
    }
    module.run(request, &[], &mut |_| {})?;
    Ok(Some(LanguageEnsureResult {
        summary: Default::default(),
        published: true,
        generation_id: None,
    }))
}

fn try_accept_or_warm_report(
    request: &EnsureRequest,
    module: &dyn LanguageRuntime,
    planned: &[String],
    witness: &Option<crate::test_runner::lang_iface::ExecutionWitness>,
    misses: &[String],
) -> Result<Option<LanguageEnsureResult>, String> {
    if misses.is_empty() {
        let w = witness.as_ref().expect("accept implies loaded witness");
        return Ok(Some(LanguageEnsureResult {
            summary: rules(module).accepted_summary(request, planned, w)?,
            published: false,
            generation_id: Some(w.generation_id.clone()),
        }));
    }

    if !request.force
        && let Some(w) = witness.as_ref()
        && all_misses_warm_skippable(w, misses)
    {
        return Ok(Some(LanguageEnsureResult {
            summary: rules(module).cached_witness_summary(request, planned, w),
            published: false,
            generation_id: Some(w.generation_id.clone()),
        }));
    }
    Ok(None)
}

fn cached_selectors_for_run(
    recap_stored: bool,
    planned: &[String],
    misses: &[String],
    witness: Option<&crate::test_runner::lang_iface::ExecutionWitness>,
) -> Vec<String> {
    let mut cached: BTreeSet<String> = planned
        .iter()
        .filter(|selector| !misses.iter().any(|miss| miss == *selector))
        .cloned()
        .collect();
    if recap_stored && let Some(stored) = witness {
        for selector in &stored.selectors {
            if !misses.iter().any(|miss| miss == selector) {
                cached.insert(selector.clone());
            }
        }
    }
    cached.into_iter().collect()
}

fn run_misses_and_maybe_publish(
    request: &EnsureRequest,
    module: &dyn LanguageRuntime,
    planned: &[String],
    witness: Option<crate::test_runner::lang_iface::ExecutionWitness>,
    misses: &[String],
) -> Result<LanguageEnsureResult, String> {
    let cached_selectors = cached_selectors_for_run(
        rules(module).recap_stored_selectors(),
        planned,
        misses,
        witness.as_ref(),
    );
    if !cached_selectors.is_empty()
        && let Some(w) = witness.as_ref()
    {
        let _ = rules(module).cached_witness_summary(request, &cached_selectors, w);
    }
    let mut records = Vec::new();
    let mut batch = module.run(request, misses, &mut |record| records.push(record))?;
    batch.summary = std::mem::take(&mut batch.summary).with_records(records);
    Ok(LanguageEnsureResult {
        summary: merge_accept_and_run(&cached_selectors, witness.as_ref(), &batch),
        published: true,
        generation_id: None,
    })
}

fn merge_accept_and_run(
    cached_selectors: &[String],
    prior: Option<&crate::test_runner::lang_iface::ExecutionWitness>,
    batch: &OutcomeBatch,
) -> crate::test_runner::runners::SelectorExecutionSummary {
    let mut summary = batch.summary.clone();
    let Some(prior) = prior else {
        return summary;
    };
    for selector in cached_selectors {
        if batch.selectors.contains(selector) {
            continue;
        }
        let Some(index) = prior.selectors.iter().position(|stored| stored == selector) else {
            continue;
        };
        let Some(status) = prior.statuses[index].to_test_status() else {
            continue;
        };
        let Some(duration_ns) = prior.durations_ns.get(index).copied().flatten() else {
            continue;
        };
        let raw_status = prior
            .raw_statuses
            .get(index)
            .and_then(|status| status.to_test_status());
        summary.record(crate::test_runner::runners::SelectorExecutionRecord {
            selector: selector.clone(),
            status,
            raw_status,
            cache_record: crate::test_runner::runners::SelectorCacheRecord::Hit,
            exit_code: Some(if status == kiss::rpytest_runner::TestStatus::Passed {
                0
            } else {
                1
            }),
            duration: std::time::Duration::from_nanos(duration_ns),
        });
    }
    summary
}

fn union_incomparable_timing_misses(
    request: &EnsureRequest,
    module: &dyn LanguageRuntime,
    planned: &[String],
    witness: &Option<crate::test_runner::lang_iface::ExecutionWitness>,
    misses: &mut Vec<String>,
) {
    if request.gate.unit_test_time_gate_disabled() || timing_context_matches(request, module) {
        return;
    }
    let Some(witness) = witness.as_ref() else {
        return;
    };
    let extra: Vec<String> = planned
        .iter()
        .filter_map(|sel| {
            let i = witness.selectors.iter().position(|s| s == sel)?;
            let raw = witness
                .raw_statuses
                .get(i)
                .copied()
                .unwrap_or(witness.statuses[i]);
            if raw == crate::test_runner::lang_iface::WitnessStatus::Passed
                && witness.durations_ns.get(i).copied().flatten().is_some()
            {
                Some(sel.clone())
            } else {
                None
            }
        })
        .collect();
    crate::test_runner::lang_iface::union_force_selectors_into_misses(planned, misses, &extra);
}

fn rules(module: &dyn LanguageRuntime) -> &'static dyn KernelRules {
    crate::test_runner::lang_registry::rules_for(module.language())
}

fn timing_context_matches(request: &EnsureRequest, module: &dyn LanguageRuntime) -> bool {
    let rules = rules(module);
    timing_context_is_comparable(
        &rules.stored_timing_digest(request),
        &rules.current_timing_digest(request),
    )
}
