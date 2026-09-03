use crate::test_runner::lang_iface::{
    AcceptMode, EnsureRequest, EnsureRuntimeResult, LanguageEnsureResult, LanguageRuntime,
    OutcomeBatch, PublishBatch, all_misses_warm_skippable, miss_selectors_for_repair,
    reclassify_statuses_with_gate, session_timing_context_digest, timing_context_is_comparable,
};
use kiss::GateConfig;
use kiss::Language;

pub(crate) fn ensure_runtime_cache(
    request: &EnsureRequest,
    modules: &[&dyn LanguageRuntime],
) -> Result<EnsureRuntimeResult, String> {
    kiss::rust_llvm_cov_runner::reset_subprocess_observer();
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
    module.bind_subprocess_observer(request);
    if let Some(empty) = try_publish_empty_all(request, module, &planned)? {
        return Ok(empty);
    }
    let identity_started = std::time::Instant::now();
    let identity = module.current_identity(request)?;
    match module.language() {
        Language::Rust => {
            crate::test_runner::emit_stage_time("rust_identity", identity_started.elapsed());
        }
        Language::Python => {
            crate::test_runner::emit_stage_time(
                "python_source_fingerprint",
                identity_started.elapsed(),
            );
        }
    }
    let loaded = match module.load_full_witness(&request.repo_root) {
        Ok(witness) => Some(witness),
        Err(err) => {
            if module.language() == Language::Rust && !err.contains("No such file") {
                eprintln!("kiss test: rust witness load: {err}");
            }
            None
        }
    };
    let mut witness = loaded.clone();
    reclassify_loaded_witness(request, module, gate, &mut witness)?;
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
    union_source_delta_misses(request, module, &planned, &mut misses)?;
    union_incomparable_timing_misses(request, module, &planned, &witness, &mut misses);
    if let Some(accepted) = try_accept_or_warm_report(request, module, &planned, &witness, &misses)?
    {
        return Ok(accepted);
    }
    run_misses_and_maybe_publish(request, module, &planned, witness, &misses)
}

fn union_source_delta_misses(
    request: &EnsureRequest,
    module: &dyn LanguageRuntime,
    planned: &[String],
    misses: &mut Vec<String>,
) -> Result<(), String> {
    let extra = module.extra_source_delta_misses(request, planned)?;
    crate::test_runner::lang_iface::union_force_selectors_into_misses(planned, misses, &extra);
    let policy = kiss::TestSectionConfig::load().cache_policy;
    let banned: Vec<String> = planned
        .iter()
        .filter(|sel| policy.is_non_cacheable(sel))
        .cloned()
        .collect();
    crate::test_runner::lang_iface::union_force_selectors_into_misses(planned, misses, &banned);
    Ok(())
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
    if !timing_context_matches(request, module.language()) {
        w.statuses = w.raw_statuses.clone();
        return Ok(());
    }
    let gate_selectors = match module.selectors_for_time_gate(request, &w.selectors) {
        Ok(selectors) => selectors,
        Err(err) if module.language() == Language::Python => return Err(err),
        Err(_) => w.selectors.clone(),
    };
    w.statuses =
        reclassify_statuses_with_gate(&gate_selectors, &w.raw_statuses, &w.durations_ns, gate);
    Ok(())
}

fn try_publish_empty_all(
    request: &EnsureRequest,
    module: &dyn LanguageRuntime,
    planned: &[String],
) -> Result<Option<LanguageEnsureResult>, String> {
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
) -> Result<Option<LanguageEnsureResult>, String> {
    if misses.is_empty() {
        let w = witness.as_ref().expect("accept implies loaded witness");
        return Ok(Some(LanguageEnsureResult {
            summary: module.accepted_summary(request, planned, w)?,
            published: false,
            generation_id: Some(w.generation_id.clone()),
        }));
    }

    if !request.force
        && let Some(w) = witness.as_ref()
        && all_misses_warm_skippable(w, misses)
    {
        return Ok(Some(LanguageEnsureResult {
            summary: module.cached_witness_summary(request, planned, w),
            published: false,
            generation_id: Some(w.generation_id.clone()),
        }));
    }
    Ok(None)
}

fn run_misses_and_maybe_publish(
    request: &EnsureRequest,
    module: &dyn LanguageRuntime,
    planned: &[String],
    witness: Option<crate::test_runner::lang_iface::ExecutionWitness>,
    misses: &[String],
) -> Result<LanguageEnsureResult, String> {
    let batch = module.run_selectors(request, misses)?;

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

    module.publish_outcomes(request, &publish)?;
    Ok(LanguageEnsureResult {
        summary: merge_accept_and_run(planned, witness.as_ref(), &batch),
        published: true,
        generation_id: None,
    })
}

fn merge_accept_and_run(
    planned: &[String],
    prior: Option<&crate::test_runner::lang_iface::ExecutionWitness>,
    batch: &OutcomeBatch,
) -> crate::test_runner::runners::SelectorExecutionSummary {
    let mut summary = batch.summary.clone();
    let Some(prior) = prior else {
        return summary;
    };
    for selector in planned {
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
    if request.gate.unit_test_time_gate_disabled()
        || timing_context_matches(request, module.language())
    {
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

fn timing_context_matches(request: &EnsureRequest, language: Language) -> bool {
    let current = match language {
        Language::Rust => session_timing_context_digest(request.jobs),
        Language::Python => session_timing_context_digest(0),
    };
    timing_context_is_comparable(&stored_timing_digest(request, language), &current)
}

fn stored_timing_digest(request: &EnsureRequest, language: Language) -> String {
    match language {
        Language::Python => session_timing_context_digest(0),
        Language::Rust => {
            let cache = crate::test_runner::rust_coverage_index::rust_coverage_cache_root(
                &request.repo_root,
            );
            crate::test_runner::execution_generation::load_current_generation(&cache)
                .ok()
                .map(|(generation, _)| generation.timing_context_digest)
                .unwrap_or_else(|| session_timing_context_digest(request.jobs))
        }
    }
}
