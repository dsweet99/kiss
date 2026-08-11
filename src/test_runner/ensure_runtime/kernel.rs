//! Generic accept → run misses → always publish kernel.

use crate::test_runner::lang_iface::{
    AcceptMode, EnsureRequest, EnsureRuntimeResult, LanguageEnsureResult, LanguageRuntime,
    OutcomeBatch, PublishBatch, miss_selectors_for_repair, reclassify_statuses_with_gate,
};
use kiss::GateConfig;
use kiss::Language;

pub(crate) fn ensure_runtime_cache(
    request: &EnsureRequest,
    modules: &[&dyn LanguageRuntime],
) -> Result<EnsureRuntimeResult, String> {
    let mut result = EnsureRuntimeResult::default();
    let gate = GateConfig::load();
    for module in modules {
        let language = module.language();
        if !request.requires(language) {
            continue;
        }
        let lang_result = ensure_one_language(request, *module, &gate)?;
        let exit = lang_result.summary.exit_code;
        match language {
            Language::Python => result.python = Some(lang_result),
            Language::Rust => result.rust = Some(lang_result),
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
    // Empty All-mode universe: publish empty Full (no accept/run).
    if planned.is_empty() && request.mode == AcceptMode::All {
        let publish = PublishBatch {
            selectors: vec![],
            statuses: vec![],
            durations_ns: vec![],
            covered_lines: Default::default(),
            publication_universe: Some(vec![]),
            summary: Default::default(),
        };
        module.publish_outcomes(request, &publish)?;
        return Ok(LanguageEnsureResult {
            summary: Default::default(),
            published: true,
            generation_id: None,
        });
    }
    let identity = module.current_identity(request)?;
    let loaded = module.load_full_witness(&request.repo_root).ok();
    let mut witness = loaded.clone();
    if let Some(ref mut w) = witness {
        w.statuses = reclassify_statuses_with_gate(
            &w.selectors,
            &w.statuses,
            &w.durations_ns,
            gate,
        );
    }
    let misses = miss_selectors_for_repair(
        request.mode,
        &planned,
        &identity,
        witness.as_ref(),
        request.force,
    );
    if misses.is_empty() {
        let w = witness.expect("accept implies loaded witness");
        let summary = module.accepted_summary(request, &planned, &w);
        return Ok(LanguageEnsureResult {
            summary,
            published: false,
            generation_id: Some(w.generation_id),
        });
    }
    let batch = module.run_selectors(request, &misses)?;
    let publish = PublishBatch {
        selectors: batch.selectors.clone(),
        statuses: batch.statuses.clone(),
        durations_ns: batch.durations_ns.clone(),
        covered_lines: batch.covered_lines.clone(),
        publication_universe: batch.publication_universe.clone().or_else(|| {
            if request.mode == AcceptMode::All {
                Some(planned.clone())
            } else {
                None
            }
        }),
        summary: batch.summary.clone(),
    };
    // Always publish collected outcomes before surfacing nonzero execution status.
    module.publish_outcomes(request, &publish)?;
    Ok(LanguageEnsureResult {
        summary: merge_accept_and_run(&planned, witness.as_ref(), &batch),
        published: true,
        generation_id: None,
    })
}

fn merge_accept_and_run(
    planned: &[String],
    prior: Option<&crate::test_runner::lang_iface::ExecutionWitness>,
    batch: &OutcomeBatch,
) -> crate::test_runner::runners::SelectorExecutionSummary {
    let _ = (planned, prior);
    batch.summary.clone()
}
