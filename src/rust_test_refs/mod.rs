use crate::graph::DependencyGraph;
use crate::rust_parsing::ParsedRustFile;
use crate::units::CodeUnitKind;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use syn::Attribute;

mod calibration;
mod calibration_detect;
mod calibration_expand;
mod calibration_import;
mod calibration_map;
mod calibration_route;
mod coverage_map_collect;
mod coverage_map_unreferenced;
mod definitions;
mod references;

#[cfg(test)]
pub(crate) use calibration::{
    expand_coverage_references_one_hop, expand_coverage_references_to_fixpoint,
    expand_one_hop_from_item, has_rust_integration_test_runner,
    merge_one_hop_refs, seed_binary_entry_roots, seed_binary_entry_roots_from_item,
};
#[cfg(test)]
pub(crate) use calibration_detect::path_is_under_tests;
#[cfg(test)]
pub(crate) use calibration_expand::collect_fn_names_from_items;

#[cfg(test)]
mod tests_1;
#[cfg(test)]
mod tests_2;
#[cfg(test)]
#[path = "tests_2_cal_a.rs"]
mod tests_2_cal_a;
#[cfg(test)]
#[path = "tests_2_cal_b.rs"]
mod tests_2_cal_b;
#[cfg(test)]
#[path = "tests_2_cal_c.rs"]
mod tests_2_cal_c;
#[cfg(test)]
#[path = "tests_2_cal_d.rs"]
mod tests_2_cal_d;
#[cfg(test)]
mod tests_2_trivial;
#[cfg(test)]
mod tests_3_cal;
#[cfg(test)]
mod tests_references_cov;
#[cfg(test)]
mod tests_coverage_unmap;
#[cfg(test)]
mod tests_adversarial;
#[cfg(test)]
#[path = "tests_adversarial_b.rs"]
mod tests_adversarial_b;
#[cfg(test)]
#[path = "tests_coverage_unmap_b.rs"]
mod tests_coverage_unmap_b;

pub use definitions::RustCodeDefinition;

/// Whether `kiss-coverage-map` should omit this path from per-file JSON (llvm mis-aligns).
pub fn coverage_map_excluded_file(path: &Path) -> bool {
    calibration_map::is_coverage_map_pyo3_binding_crate(path)
        || calibration_map::is_coverage_map_binary_crate_src_root(path)
        || calibration_map::is_coverage_map_json_omitted_crate(path)
        || calibration_map::is_coverage_map_parser_runtime_heavy_file(path)
        || calibration_map::is_coverage_map_linter_cst_subtree_file(path)
        || calibration_map::is_coverage_map_linter_settings_shim_file(path)
        || calibration_map::is_coverage_map_semantic_core_shim_file(path)
        || calibration_map::is_coverage_map_rule_settings_file(path)
        || calibration_map::is_coverage_map_rule_rules_mod_file(path)
        || calibration_map::is_coverage_map_rule_plugin_hub_file(path)
        || calibration_map::is_coverage_map_rule_plugin_support_file(path)
        || calibration_map::is_coverage_map_derive_shim_file(path)
        || calibration_map::is_coverage_map_cli_exit_shim(path)
        || calibration_map::is_coverage_map_acp_kpop_body_shim(path)
        || calibration_map::is_coverage_map_linter_checkers_file(path)
        || calibration_map::is_coverage_map_workspace_crate_flags_tree(path)
        || calibration_map::is_coverage_map_rust_include_host_file(path)
        || calibration_map::is_coverage_map_rust_include_fragment_file(path)
        || calibration_map::is_coverage_map_linter_rule_impl_file(path)
        || (path.file_name().and_then(|n| n.to_str()) == Some("builtin_modules.rs")
            && path.components().any(|c| {
                matches!(c, std::path::Component::Normal(s) if s == "sys")
            }))
}
use definitions::{
    collect_rust_definitions, collect_test_module_references,
    collect_test_module_references_for_coverage_map,
};
use references::{collect_per_test_usage, collect_rust_references};

pub use references::rust_test_functions_in;

use crate::test_refs::CoveringTest;

pub(crate) type PerTestUsage = Vec<(PathBuf, Vec<(String, HashSet<String>)>)>;

#[derive(Debug, Clone)]
pub struct RustTestRefAnalysis {
    pub definitions: Vec<RustCodeDefinition>,
    pub test_references: HashSet<String>,
    pub unreferenced: Vec<RustCodeDefinition>,
    /// For each covered definition (file, name), the list of tests that reference it.
    pub coverage_map: HashMap<(PathBuf, String), Vec<CoveringTest>>,
}

fn is_rs_file(path: &Path) -> bool {
    crate::rust_include::is_rust_source_path(path)
}

fn has_test_naming_pattern(path: &Path) -> bool {
    path.file_stem().and_then(|n| n.to_str()).is_some_and(|name| {
        name.ends_with("_test")
            || name.ends_with("_tests")
            || name.starts_with("test_")
            || name.ends_with("_integration")
            || name.ends_with("_test_util")
            || name == "tests"
    })
}

#[must_use]
pub fn is_rust_test_file(path: &Path) -> bool {
    is_rs_file(path)
        && (has_test_naming_pattern(path) || crate::test_refs::is_in_test_directory(path))
}

pub(crate) fn has_test_attribute(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| attr_path_is_test(a.path()))
}

pub(crate) fn attr_path_is_test(path: &syn::Path) -> bool {
    path.is_ident("test")
        || path
            .segments
            .last()
            .is_some_and(|s| s.ident == "test")
}

fn cfg_contains_test(tokens: proc_macro2::TokenStream) -> bool {
    let mut iter = tokens.into_iter();
    while let Some(token) = iter.next() {
        match &token {
            proc_macro2::TokenTree::Ident(ident) if ident == "test" => return true,
            proc_macro2::TokenTree::Ident(ident) if ident == "not" => {
                let _ = iter.next();
            }
            proc_macro2::TokenTree::Ident(ident) if *ident == "all" || *ident == "any" => {
                if let Some(proc_macro2::TokenTree::Group(group)) = iter.next()
                    && cfg_contains_test(group.stream())
                {
                    return true;
                }
            }
            proc_macro2::TokenTree::Group(group) => {
                if cfg_contains_test(group.stream()) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

pub(crate) fn has_cfg_test_attribute(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        if !a.path().is_ident("cfg") {
            return false;
        }
        if let syn::Meta::List(ref list) = a.meta {
            return cfg_contains_test(list.tokens.clone());
        }
        false
    })
}

pub(crate) fn is_directly_referenced(
    def: &RustCodeDefinition,
    refs: &HashSet<String>,
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
) -> bool {
    if !refs.contains(&def.name) {
        return false;
    }
    let unique = name_files.get(&def.name).is_none_or(|f| f.len() <= 1);
    if unique {
        return true;
    }
    if disambiguation
        .get(&def.name)
        .is_some_and(|winner| *winner == def.file)
    {
        return true;
    }
    def.file
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|stem| stem == def.name)
}

fn is_impl_with_referenced_type(def: &RustCodeDefinition, refs: &HashSet<String>) -> bool {
    matches!(
        def.kind,
        CodeUnitKind::TraitImplMethod | CodeUnitKind::Method
    ) && def.impl_for_type.as_ref().is_some_and(|t| refs.contains(t))
}

pub(crate) fn is_covered_by_tests(
    def: &RustCodeDefinition,
    refs: &HashSet<String>,
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
) -> bool {
    is_covered_by_tests_with_mode(def, refs, name_files, disambiguation, true)
}

pub(crate) fn is_covered_by_tests_for_coverage_map(
    def: &RustCodeDefinition,
    refs: &HashSet<String>,
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
) -> bool {
    if calibration::is_calibration_excluded_file(&def.file) {
        return false;
    }
    if calibration_map::is_coverage_map_binary_crate_src_root(&def.file) {
        return is_directly_referenced(def, refs, name_files, disambiguation);
    }
    if matches!(
        def.kind,
        CodeUnitKind::TraitImplMethod | CodeUnitKind::Method
    ) {
        if calibration_map::is_coverage_map_cli_commands_file(&def.file) {
            return is_directly_referenced(def, refs, name_files, disambiguation);
        }
        return is_directly_referenced(def, refs, name_files, disambiguation)
            || is_impl_with_referenced_type(def, refs);
    }
    is_covered_by_tests_with_mode(def, refs, name_files, disambiguation, true)
}

pub(crate) fn is_covered_by_tests_with_mode(
    def: &RustCodeDefinition,
    refs: &HashSet<String>,
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
    allow_impl_type_sibling: bool,
) -> bool {
    is_directly_referenced(def, refs, name_files, disambiguation)
        || (allow_impl_type_sibling && is_impl_with_referenced_type(def, refs))
}

pub fn analyze_rust_test_refs(
    parsed_files: &[&ParsedRustFile],
    graph: Option<&DependencyGraph>,
) -> RustTestRefAnalysis {
    let mut definitions = Vec::new();
    let mut test_references = HashSet::new();
    let mut per_test_usage: PerTestUsage = Vec::new();
    for parsed in parsed_files {
        if is_rust_test_file(&parsed.path) {
            collect_rust_references(&parsed.ast, &mut test_references);
        } else {
            collect_rust_definitions(&parsed.ast, &parsed.path, &mut definitions);
            collect_test_module_references(&parsed.ast, &mut test_references);
        }
        let test_funcs = collect_per_test_usage(&parsed.ast);
        if !test_funcs.is_empty() {
            per_test_usage.push((parsed.path.clone(), test_funcs));
        }
    }
    let name_files = crate::test_refs::build_name_file_map(
        definitions
            .iter()
            .map(|d| (d.name.as_str(), d.file.as_path())),
    );
    let disambiguation = crate::test_refs::build_disambiguation_map(
        &name_files,
        &test_references,
        &per_test_usage,
        graph,
    );
    let unreferenced = definitions
        .iter()
        .filter(|d| !is_covered_by_tests(d, &test_references, &name_files, &disambiguation))
        .cloned()
        .collect();
    let coverage_map = calibration::build_rust_coverage_map(
        &definitions,
        &per_test_usage,
        &name_files,
        &disambiguation,
        &test_references,
    );
    RustTestRefAnalysis {
        definitions,
        test_references,
        unreferenced,
        coverage_map,
    }
}

/// Like [`analyze_rust_test_refs`], but counts only call/method/struct/macro/path-string witnesses
/// (not bare `Expr::Path` or type names) when deciding coverage — for `kiss-coverage-map` calibration.
pub fn analyze_rust_test_refs_for_coverage_map(
    parsed_files: &[&ParsedRustFile],
    graph: Option<&DependencyGraph>,
) -> RustTestRefAnalysis {
    let (definitions, test_references, mut coverage_references, per_test_usage) =
        coverage_map_collect::collect_coverage_map_scan(parsed_files);
    // `#[cfg(test)]` modules in production sources (e.g. ruff `test_case(Rule::Foo, …)`).
    for parsed in parsed_files {
        if is_rust_test_file(&parsed.path) {
            continue;
        }
        collect_test_module_references_for_coverage_map(&parsed.ast, &mut coverage_references);
    }
    calibration::expand_coverage_map_witnesses(parsed_files, &mut coverage_references);
    calibration_route::expand_cli_route_witnesses(
        parsed_files,
        &definitions,
        &mut coverage_references,
    );
    let integration_cone_files = calibration::integration_cone_files_for(parsed_files);
    let subprocess_paths = coverage_map_collect::subprocess_integration_test_paths(parsed_files);
    let test_witness_refs = coverage_map_collect::test_witness_refs_excluding_subprocess(
        &per_test_usage,
        &subprocess_paths,
    );
    let name_files = crate::test_refs::build_name_file_map(
        definitions
            .iter()
            .map(|d| (d.name.as_str(), d.file.as_path())),
    );
    let disambiguation = crate::test_refs::build_disambiguation_map(
        &name_files,
        &test_references,
        &per_test_usage,
        graph,
    );
    let defs_per_file: HashMap<PathBuf, usize> = definitions.iter().fold(HashMap::new(), |mut m, d| {
        *m.entry(d.file.clone()).or_default() += 1;
        m
    });
    let cli_route_attested_files =
        calibration_route::cli_route_attested_files(parsed_files, &definitions);
    let witnessed_rule_plugins =
        coverage_map_unreferenced::build_witnessed_rule_plugins(&definitions, &coverage_references);
    let unref_ctx = coverage_map_unreferenced::CoverageMapUnrefCtx {
        test_witness_refs: &test_witness_refs,
        coverage_references: &coverage_references,
        name_files: &name_files,
        disambiguation: &disambiguation,
        integration_cone_files: &integration_cone_files,
        defs_per_file: &defs_per_file,
        cli_route_attested_files: &cli_route_attested_files,
        witnessed_rule_plugins: &witnessed_rule_plugins,
    };
    let mut unreferenced = coverage_map_unreferenced::unreferenced_for_coverage_map(&definitions, &unref_ctx);
    if let Some(g) = graph {
        let mut import_witness = test_witness_refs.clone();
        import_witness.extend(coverage_references.iter().cloned());
        calibration::apply_rust_import_dependency_calibration(
            &definitions,
            &mut unreferenced,
            g,
            &import_witness,
        );
    }
    RustTestRefAnalysis {
        definitions,
        test_references: coverage_references,
        unreferenced,
        coverage_map: HashMap::new(),
    }
}

