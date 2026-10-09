use crate::test_runner::lang_iface::{
    AcceptMode, EnsureRequest, EnsureRuntimeResult, KernelRules, LanguageEnsureResult,
    LanguageRuntime,
};

pub(crate) fn ensure_runtime_cache(
    request: &EnsureRequest,
    modules: &[&dyn LanguageRuntime],
) -> Result<EnsureRuntimeResult, String> {
    kiss::subprocess_observer::reset_subprocess_observer();
    let mut result = EnsureRuntimeResult::default();
    for module in modules {
        let language = module.language();
        if !request.requires(language) {
            continue;
        }
        let lang_result = ensure_one_language(request, *module)?;
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
) -> Result<LanguageEnsureResult, String> {
    rules(module).bind_subprocess_observer(request);
    if let Some(empty) = try_publish_empty_all(request, module)? {
        return Ok(empty);
    }
    let listing = timed_list(request, module)?;
    run_planned(request, module, &listing.ids)
}

fn run_planned(
    request: &EnsureRequest,
    module: &dyn LanguageRuntime,
    planned: &[String],
) -> Result<LanguageEnsureResult, String> {
    let mut records = Vec::new();
    let mut batch = module.run(request, planned, &mut |record| records.push(record))?;
    batch.summary = std::mem::take(&mut batch.summary).with_records(records);
    Ok(LanguageEnsureResult {
        summary: batch.summary,
        published: true,
        generation_id: None,
    })
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

fn rules(module: &dyn LanguageRuntime) -> &'static dyn KernelRules {
    crate::test_runner::lang_registry::rules_for(module.language())
}
