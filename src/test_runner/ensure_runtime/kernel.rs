//! Generic accept → run misses → always publish kernel.

use crate::test_runner::lang_iface::{
    AcceptMode, EnsureRequest, EnsureRuntimeResult, LanguageEnsureResult, LanguageRuntime,
    OutcomeBatch, PublishBatch, all_misses_warm_skippable, miss_selectors_for_repair,
    reclassify_statuses_with_gate,
};
use kiss::GateConfig;
use kiss::Language;

pub(crate) fn ensure_runtime_cache(
    request: &EnsureRequest,
    modules: &[&dyn LanguageRuntime],
) -> Result<EnsureRuntimeResult, String> {
    let mut result = EnsureRuntimeResult::default();
    let gate = &request.gate;
    for module in modules {
        let language = module.language();
        if !request.requires(language) {
            continue;
        }
        let lang_result = ensure_one_language(request, *module, gate)?;
        let exit = lang_result.summary.exit_code;
        match language {
            Language::Python => result.by_language.python = Some(lang_result),
            Language::Rust => result.by_language.rust = Some(lang_result),
        }
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
    let planned = request.planned_for(module.language()).to_vec();
    if let Some(empty) = try_publish_empty_all(request, module, &planned)? {
        return Ok(empty);
    }
    let identity_started = std::time::Instant::now();
    let identity = module.current_identity(request)?;
    if module.language() == Language::Rust {
        crate::test_runner::emit_stage_time("rust_identity", identity_started.elapsed());
    }
    let loaded = module.load_full_witness(&request.repo_root).ok();
    let mut witness = loaded.clone();
    if let Some(ref mut w) = witness {
        let gate_selectors = module.selectors_for_time_gate(request, &w.selectors)?;
        w.statuses = reclassify_statuses_with_gate(
            &gate_selectors,
            &w.statuses,
            &w.durations_ns,
            gate,
        );
    }
    let mut misses = miss_selectors_for_repair(
        request.mode,
        &planned,
        &identity,
        witness.as_ref(),
        request.force,
    );
    crate::test_runner::lang_iface::union_force_selectors_into_misses(
        &planned,
        &mut misses,
        &request.force_selectors,
    );
    if let Some(accepted) = try_accept_or_warm_report(request, module, &planned, &witness, &misses) {
        return Ok(accepted);
    }
    run_misses_and_maybe_publish(request, module, &planned, witness, &misses)
}

fn try_publish_empty_all(
    request: &EnsureRequest,
    module: &dyn LanguageRuntime,
    planned: &[String],
) -> Result<Option<LanguageEnsureResult>, String> {
    // Empty All-mode universe: publish empty Full (no accept/run).
    if !(planned.is_empty() && request.mode == AcceptMode::All) {
        return Ok(None);
    }
    let publish = PublishBatch {
        selectors: vec![],
        statuses: vec![],
        durations_ns: vec![],
        covered_lines: Default::default(),
        publication_universe: Some(vec![]),
        summary: Default::default(),
    };
    module.publish_outcomes(request, &publish)?;
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
) -> Option<LanguageEnsureResult> {
    if misses.is_empty() {
        let w = witness.as_ref().expect("accept implies loaded witness");
        return Some(LanguageEnsureResult {
            summary: module.accepted_summary(request, planned, w),
            published: false,
            generation_id: Some(w.generation_id.clone()),
        });
    }
    // Warm incomplete: TimedOut/Unresolved stay reported without runner re-entry
    // unless --force. Failed still repairs. (sameq generations can leave timeout
    // slots as unresolved forever otherwise.)
    if !request.force
        && let Some(w) = witness.as_ref()
        && all_misses_warm_skippable(w, misses)
    {
        return Some(LanguageEnsureResult {
            summary: module.cached_witness_summary(request, planned, w),
            published: false,
            generation_id: Some(w.generation_id.clone()),
        });
    }
    None
}

fn run_misses_and_maybe_publish(
    request: &EnsureRequest,
    module: &dyn LanguageRuntime,
    planned: &[String],
    witness: Option<crate::test_runner::lang_iface::ExecutionWitness>,
    misses: &[String],
) -> Result<LanguageEnsureResult, String> {
    let batch = module.run_selectors(request, misses)?;
    // Full All-mode cold runs set publication_universe to the planned universe.
    // Partial incomplete repairs leave it None so languages delta-repair instead of
    // rebuilding the whole derived population (sameq-scale line indexes).
    let publication_universe = batch.publication_universe.clone().or_else(|| {
        if request.mode == AcceptMode::All && batch.selectors.len() == planned.len() {
            Some(planned.to_vec())
        } else {
            None
        }
    });
    let publish = PublishBatch {
        selectors: batch.selectors.clone(),
        statuses: batch.statuses.clone(),
        durations_ns: batch.durations_ns.clone(),
        covered_lines: batch.covered_lines.clone(),
        publication_universe,
        summary: batch.summary.clone(),
    };
    // Skip republish when repair only re-read unchanged cache hits (common for
    // incomplete generations whose TimedOut/Failed selectors remain terminal).
    if outcomes_unchanged_vs_prior(witness.as_ref(), &batch) {
        return Ok(LanguageEnsureResult {
            summary: merge_accept_and_run(planned, witness.as_ref(), &batch),
            published: false,
            generation_id: witness.map(|w| w.generation_id),
        });
    }
    // Always publish collected outcomes before surfacing nonzero execution status.
    module.publish_outcomes(request, &publish)?;
    Ok(LanguageEnsureResult {
        summary: merge_accept_and_run(planned, witness.as_ref(), &batch),
        published: true,
        generation_id: None,
    })
}

fn outcomes_unchanged_vs_prior(
    prior: Option<&crate::test_runner::lang_iface::ExecutionWitness>,
    batch: &crate::test_runner::lang_iface::OutcomeBatch,
) -> bool {
    let Some(prior) = prior else {
        return false;
    };
    let index = prior
        .selectors
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect::<std::collections::BTreeMap<_, _>>();
    batch.selectors.iter().zip(batch.statuses.iter()).zip(batch.durations_ns.iter()).all(
        |((sel, status), dur)| match index.get(sel.as_str()) {
            Some(&i) => prior.statuses[i] == *status && prior.durations_ns[i] == *dur,
            None => false,
        },
    )
}

fn merge_accept_and_run(
    planned: &[String],
    prior: Option<&crate::test_runner::lang_iface::ExecutionWitness>,
    batch: &OutcomeBatch,
) -> crate::test_runner::runners::SelectorExecutionSummary {
    let _ = (planned, prior);
    batch.summary.clone()
}
