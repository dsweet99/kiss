use super::coverage::is_definition_covered_for_calibration;
use super::coverage_expand::is_py_contrib_refactor_void_force_uncovered;
use super::{is_definition_covered, CodeDefinition, ParsedFile};
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
    let defs_per_file = defs_per_file_counts(definitions);
    (strict, expanded, defs_per_file)
}

pub(crate) struct UnreferencedFilterCtx<'a> {
    pub calibration: bool,
    pub defs_per_file: &'a HashMap<PathBuf, usize>,
    pub test_witness_refs: &'a HashSet<String>,
    pub calibration_strict_refs: &'a HashSet<String>,
    pub calibration_expanded_refs: &'a HashSet<String>,
    pub usage_references: &'a HashSet<String>,
    pub name_files: &'a HashMap<String, HashSet<PathBuf>>,
    pub disambiguation: &'a HashMap<String, PathBuf>,
    pub import_bindings: &'a HashMap<String, HashSet<String>>,
    pub module_suffixes: &'a HashMap<PathBuf, String>,
}

pub(crate) fn filter_unreferenced_definitions(
    definitions: &[CodeDefinition],
    ctx: &UnreferencedFilterCtx<'_>,
) -> Vec<CodeDefinition> {
    definitions
        .iter()
        .filter(|def| {
            let covered = if ctx.calibration && is_py_contrib_refactor_void_force_uncovered(&def.file) {
                false
            } else if ctx.calibration {
                let file_defs = ctx.defs_per_file.get(&def.file).copied().unwrap_or(0);
                let refs = if file_defs
                    > super::coverage_expand::MAX_PRODUCTION_IMPORT_EXPAND_DEFS
                {
                    ctx.test_witness_refs
                } else if file_defs > super::coverage_expand::MAX_SAME_FILE_ONE_HOP_DEFS {
                    ctx.calibration_strict_refs
                } else {
                    ctx.calibration_expanded_refs
                };
                is_definition_covered_for_calibration(
                    def,
                    ctx.name_files,
                    ctx.disambiguation,
                    ctx.import_bindings,
                    ctx.module_suffixes,
                    refs,
                )
            } else {
                is_definition_covered(
                    def,
                    ctx.name_files,
                    ctx.disambiguation,
                    ctx.import_bindings,
                    ctx.module_suffixes,
                    ctx.usage_references,
                )
            };
            !covered
        })
        .cloned()
        .collect()
}
