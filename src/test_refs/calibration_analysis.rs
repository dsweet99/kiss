use super::coverage::{
    is_definition_covered, is_definition_covered_for_calibration, is_py_base_explicit_import_witnessed,
    is_py_oi_module_import_witnessed, is_py_package_init_import_witnessed,
};
use super::coverage_expand::{
    is_py_contrib_refactor_void_force_uncovered, is_py_experiments_path,
    is_py_inflator_calibration_path, is_py_ecosystem_auxiliary_path, is_py_inflator_call_only_path,
    is_py_inflator_denominator_path, is_py_optimizer_path,
    is_py_rl_integration_path,
};
use super::coverage_expand::is_py_base_oi_subtree;
use super::{CodeDefinition, ParsedFile};
use crate::units::CodeUnitKind;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub(crate) fn defs_per_file_counts(definitions: &[CodeDefinition]) -> HashMap<PathBuf, usize> {
    definitions.iter().fold(HashMap::new(), |mut m, d| {
        *m.entry(d.file.clone()).or_default() += 1;
        m
    })
}

pub(crate) fn build_calibration_coverage_refs(
    parsed_files: &[&ParsedFile],
    definitions: &[CodeDefinition],
    test_witness_refs: &HashSet<String>,
) -> (HashSet<String>, HashSet<String>, HashMap<PathBuf, usize>) {
    let mut strict = test_witness_refs.clone();
    super::coverage_expand::expand_py_path_literal_file_witnesses(
        parsed_files,
        definitions,
        &mut strict,
    );
    super::coverage_expand::expand_py_refs_via_production_imports(
        parsed_files,
        definitions,
        &mut strict,
    );
    let mut expanded = strict.clone();
    super::coverage_expand::expand_py_same_file_one_hop(parsed_files, definitions, &mut expanded);
    super::coverage_expand::expand_py_witnessed_directory_sibling_defs(definitions, &mut expanded);
    // Epoch-2: OI interface downward closure after expanded refs are fixed (g10 idea #2).
    super::coverage_expand::expand_py_oi_interface_downward_witnesses(definitions, &mut expanded);
    let defs_per_file = defs_per_file_counts(definitions);
    (strict, expanded, defs_per_file)
}

/// Which ref tier to apply when filtering calibration coverage (idea #8 interval bounds).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum CalibrationCoverageBound {
    /// Per-path tier selection used by shipped `kiss-coverage-map`.
    Shipped,
    /// Global strict refs only — attested lower bound.
    Attested,
    /// Global expanded refs only — optimistic upper bound.
    Optimistic,
}

pub(crate) struct UnreferencedFilterCtx<'a> {
    pub calibration: bool,
    pub coverage_bound: CalibrationCoverageBound,
    pub defs_per_file: &'a HashMap<PathBuf, usize>,
    pub test_witness_refs: &'a HashSet<String>,
    pub calibration_strict_refs: &'a HashSet<String>,
    pub calibration_expanded_refs: &'a HashSet<String>,
    pub void_dispatch_attestation: &'a HashMap<PathBuf, HashSet<String>>,
    pub usage_references: &'a HashSet<String>,
    pub name_files: &'a HashMap<String, HashSet<PathBuf>>,
    pub disambiguation: &'a HashMap<String, PathBuf>,
    pub import_bindings: &'a HashMap<String, HashSet<String>>,
    pub module_suffixes: &'a HashMap<PathBuf, String>,
}

fn void_partition_covered(def: &CodeDefinition, ctx: &UnreferencedFilterCtx<'_>) -> bool {
    let Some(allowed) = ctx.void_dispatch_attestation.get(&def.file) else {
        return false;
    };
    let name_ok = allowed.is_empty() || allowed.contains(&def.name);
    name_ok
        && is_definition_covered_for_calibration(
            def,
            ctx.name_files,
            ctx.disambiguation,
            ctx.import_bindings,
            ctx.module_suffixes,
            ctx.calibration_strict_refs,
        )
}

fn inflator_call_only_uncovered(def: &CodeDefinition, ctx: &UnreferencedFilterCtx<'_>) -> bool {
    is_py_inflator_denominator_path(&def.file)
        && !is_py_optimizer_path(&def.file)
        && !ctx.test_witness_refs.contains(&def.name)
}

fn ops_hub_class_deny(def: &CodeDefinition, file_defs: usize) -> bool {
    let ops_inflator = is_py_inflator_calibration_path(&def.file)
        && !is_py_optimizer_path(&def.file)
        && !is_py_experiments_path(&def.file)
        && !is_py_inflator_call_only_path(&def.file);
    ops_inflator
        && def.kind == CodeUnitKind::Class
        && file_defs > super::coverage_expand::MAX_DIR_SIBLING_EXPAND_DEFS
}

fn bound_tier_covered(
    def: &CodeDefinition,
    ctx: &UnreferencedFilterCtx<'_>,
    refs: &HashSet<String>,
) -> bool {
    is_py_package_init_import_witnessed(def, ctx.import_bindings, ctx.module_suffixes)
        || is_definition_covered_for_calibration(
            def,
            ctx.name_files,
            ctx.disambiguation,
            ctx.import_bindings,
            ctx.module_suffixes,
            refs,
        )
}

fn experiments_path_covered(def: &CodeDefinition, ctx: &UnreferencedFilterCtx<'_>) -> bool {
    ctx.test_witness_refs.contains(&def.name)
        && is_definition_covered_for_calibration(
            def,
            ctx.name_files,
            ctx.disambiguation,
            ctx.import_bindings,
            ctx.module_suffixes,
            ctx.calibration_strict_refs,
        )
}

fn rl_or_oi_path_covered(def: &CodeDefinition, ctx: &UnreferencedFilterCtx<'_>) -> bool {
    is_py_oi_module_import_witnessed(def, ctx.import_bindings, ctx.module_suffixes)
        || is_py_package_init_import_witnessed(def, ctx.import_bindings, ctx.module_suffixes)
        || is_definition_covered_for_calibration(
            def,
            ctx.name_files,
            ctx.disambiguation,
            ctx.import_bindings,
            ctx.module_suffixes,
            ctx.calibration_expanded_refs,
        )
}

fn shipped_calibration_refs<'a>(
    def: &CodeDefinition,
    ctx: &'a UnreferencedFilterCtx<'_>,
    file_defs: usize,
) -> &'a HashSet<String> {
    if is_py_optimizer_path(&def.file) || is_py_inflator_call_only_path(&def.file) {
        return ctx.calibration_strict_refs;
    }
    if is_py_ecosystem_auxiliary_path(&def.file)
        || is_py_experiments_path(&def.file)
        || is_py_inflator_calibration_path(&def.file)
        || (super::coverage_expand::is_py_base_subtree_only(&def.file) && !is_py_base_oi_subtree(&def.file))
        || file_defs > super::coverage_expand::MAX_PRODUCTION_IMPORT_EXPAND_DEFS
    {
        return ctx.calibration_strict_refs;
    }
    if file_defs > super::coverage_expand::MAX_SAME_FILE_ONE_HOP_DEFS {
        return ctx.calibration_strict_refs;
    }
    ctx.calibration_expanded_refs
}

fn shipped_calibration_covered(
    def: &CodeDefinition,
    ctx: &UnreferencedFilterCtx<'_>,
    file_defs: usize,
) -> bool {
    if is_py_experiments_path(&def.file) {
        return experiments_path_covered(def, ctx);
    }
    if is_py_rl_integration_path(&def.file) || is_py_base_oi_subtree(&def.file) {
        return rl_or_oi_path_covered(def, ctx);
    }
    let refs = shipped_calibration_refs(def, ctx, file_defs);
    is_py_package_init_import_witnessed(def, ctx.import_bindings, ctx.module_suffixes)
        || is_definition_covered_for_calibration(
            def,
            ctx.name_files,
            ctx.disambiguation,
            ctx.import_bindings,
            ctx.module_suffixes,
            refs,
        )
}

fn calibration_def_covered(def: &CodeDefinition, ctx: &UnreferencedFilterCtx<'_>) -> bool {
    let file_defs = ctx.defs_per_file.get(&def.file).copied().unwrap_or(0);
    if is_py_base_explicit_import_witnessed(
        def,
        ctx.import_bindings,
        ctx.module_suffixes,
        ctx.test_witness_refs,
    ) {
        return true;
    }
    if ctx.coverage_bound == CalibrationCoverageBound::Attested {
        return bound_tier_covered(def, ctx, ctx.calibration_strict_refs);
    }
    if ctx.coverage_bound == CalibrationCoverageBound::Optimistic {
        return bound_tier_covered(def, ctx, ctx.calibration_expanded_refs);
    }
    if inflator_call_only_uncovered(def, ctx) || ops_hub_class_deny(def, file_defs) {
        return false;
    }
    shipped_calibration_covered(def, ctx, file_defs)
}

fn definition_is_covered(def: &CodeDefinition, ctx: &UnreferencedFilterCtx<'_>) -> bool {
    if ctx.calibration && is_py_contrib_refactor_void_force_uncovered(&def.file) {
        return void_partition_covered(def, ctx);
    }
    if ctx.calibration {
        return calibration_def_covered(def, ctx);
    }
    is_definition_covered(
        def,
        ctx.name_files,
        ctx.disambiguation,
        ctx.import_bindings,
        ctx.module_suffixes,
        ctx.usage_references,
    )
}

pub(crate) fn filter_unreferenced_definitions(
    definitions: &[CodeDefinition],
    ctx: &UnreferencedFilterCtx<'_>,
) -> Vec<CodeDefinition> {
    definitions
        .iter()
        .filter(|def| !definition_is_covered(def, ctx))
        .cloned()
        .collect()
}
