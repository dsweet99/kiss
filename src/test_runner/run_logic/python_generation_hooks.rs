//! Python population generation publication hooks for language execution.

use crate::test_runner::coverage_decision::RunContext;
use crate::test_runner::python_coverage_index::{
    GenerationReason, current_complete_generation_matches, load_current_python_coverage_index,
    publish_python_derived_state_with_filter, publish_python_derived_state_with_filter_force,
    python_population_manifest_is_current_for_args_with_env_keys, repair_python_population_generation,
    selector_deltas_from_cached_outcomes, try_load_pinned_python_generation,
    PYTHON_COVERAGE_ENV_KEYS, current_python_execution_identity,
};
use crate::test_runner::runners::python_backer::PythonModule;
use std::path::Path;

pub(super) fn rebuild_python_index(
    module: &PythonModule,
    ctx: &RunContext<'_, '_>,
) -> Result<(), String> {
    if ctx.planned.skip_python_index_rebuild_after_selective {
        return Ok(());
    }
    let selective = ctx.planned.py_sel.clone();
    if try_selective_generation_repair(module, ctx, &selective)? {
        return Ok(());
    }
    publish_python_derived_state_with_filter(
        &ctx.planned.repo_root,
        None,
        ctx.options.python_extra,
        |path, repo_root| is_indexable(module, path, repo_root),
    )?;
    Ok(())
}

pub(super) fn write_python_manifest(
    module: &PythonModule,
    selectors: &[String],
    ctx: &RunContext<'_, '_>,
) -> Result<(), String> {
    if !ctx.options.force_rerun && generation_already_current(ctx, selectors) {
        return Ok(());
    }
    if ctx.options.force_rerun {
        publish_python_derived_state_with_filter_force(
            &ctx.planned.repo_root,
            Some(selectors),
            ctx.options.python_extra,
            true,
            |path, repo_root| is_indexable(module, path, repo_root),
        )?;
    } else {
        publish_python_derived_state_with_filter(
            &ctx.planned.repo_root,
            Some(selectors),
            ctx.options.python_extra,
            |path, repo_root| is_indexable(module, path, repo_root),
        )?;
    }
    Ok(())
}

pub(super) fn generation_already_current(ctx: &RunContext<'_, '_>, selectors: &[String]) -> bool {
    if current_complete_generation_matches(
        &ctx.planned.repo_root,
        selectors,
        ctx.options.python_extra,
    ) && load_current_python_coverage_index(&ctx.planned.repo_root).is_some()
    {
        return true;
    }
    if python_population_manifest_is_current_for_args_with_env_keys(
        &ctx.planned.repo_root,
        selectors,
        ctx.options.python_extra,
        PYTHON_COVERAGE_ENV_KEYS,
    ) && load_current_python_coverage_index(&ctx.planned.repo_root).is_some()
        && try_load_pinned_python_generation(&ctx.planned.repo_root).is_ok()
    {
        return true;
    }
    false
}

fn try_selective_generation_repair(
    module: &PythonModule,
    ctx: &RunContext<'_, '_>,
    selective: &[String],
) -> Result<bool, String> {
    let Ok(pinned) = try_load_pinned_python_generation(&ctx.planned.repo_root) else {
        return Ok(false);
    };
    let exec = current_python_execution_identity(&ctx.planned.repo_root, ctx.options.python_extra)?;
    if pinned.plan.base_identity != exec {
        return Ok(false);
    }
    if !selective
        .iter()
        .all(|s| pinned.plan.selectors.iter().any(|p| p == s))
    {
        return Ok(false);
    }
    let deltas = selector_deltas_from_cached_outcomes(
        &ctx.planned.repo_root,
        selective,
        ctx.options.python_extra,
        &|path, repo_root| is_indexable(module, path, repo_root),
    )?;
    let _ = repair_python_population_generation(
        &ctx.planned.repo_root,
        &deltas,
        GenerationReason::SelectiveRepair,
    )?;
    Ok(true)
}

pub(super) fn is_indexable(module: &PythonModule, path: &Path, repo_root: &Path) -> bool {
    crate::test_runner::python_coverage_index::repo_relative_coverage_file(
        repo_root,
        &path.to_string_lossy(),
    )
    .is_some()
        && {
            let _ = module;
            true
        }
}

#[cfg(test)]
#[path = "python_generation_hooks_test.rs"]
mod tests;
