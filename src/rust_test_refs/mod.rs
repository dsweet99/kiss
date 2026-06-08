use crate::graph::DependencyGraph;
use crate::rust_parsing::ParsedRustFile;
use crate::units::CodeUnitKind;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use syn::Attribute;

mod coverage;
mod coverage_map;
mod scope;
mod definitions;
mod propagation;
mod references;
mod trivial_expr;

#[cfg(test)]
mod tests_coverage;
#[cfg(test)]
mod tests_coverage_witness;

#[cfg(test)]
mod tests_1;
#[cfg(test)]
mod tests_2;
#[cfg(test)]
mod tests_vault;

pub use coverage::compute_rs_weighted_file_pcts;
pub use definitions::RustCodeDefinition;
use definitions::{
    collect_inline_test_module_witnesses, collect_rust_definitions, collect_test_module_references,
};
use coverage_map::build_rust_coverage_map;
use propagation::propagate_transitive_production_refs;
use references::{
    collect_per_test_usage, collect_rust_call_references, collect_rust_references,
    QualifiedModuleRef,
};

pub use references::rust_test_functions_in;

use crate::test_refs::disambiguation::crate_qualified_module_matches_def;
use crate::test_refs::file_to_module_suffix;
use crate::test_refs::CoveringTest;

type PerTestUsage = Vec<(PathBuf, Vec<(String, HashSet<String>)>)>;

#[derive(Debug, Clone)]
pub struct RustTestRefAnalysis {
    pub definitions: Vec<RustCodeDefinition>,
    pub test_references: HashSet<String>,
    pub call_references: HashSet<String>,
    pub propagated_references: HashSet<String>,
    pub unreferenced: Vec<RustCodeDefinition>,
    /// For each covered definition (file, name), the list of tests that reference it.
    pub coverage_map: HashMap<(PathBuf, String), Vec<CoveringTest>>,
}

fn is_rs_file(path: &Path) -> bool {
    crate::rust_include::is_rust_source_path(path)
}

fn has_test_naming_pattern(path: &Path) -> bool {
    path.file_stem()
        .and_then(|n| n.to_str())
        .is_some_and(|name| {
            name.ends_with("_test") || name.starts_with("test_") || name.ends_with("_integration")
        })
}

#[must_use]
pub fn is_rust_test_file(path: &Path) -> bool {
    is_rs_file(path)
        && (has_test_naming_pattern(path) || crate::test_refs::is_in_test_directory(path))
}

pub(crate) fn has_test_attribute(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("test"))
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

fn is_directly_referenced(
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
    if let Some(winner) = disambiguation.get(&def.name) {
        return *winner == def.file;
    }
    false
}

fn is_impl_method_covered_by_type_and_name(
    def: &RustCodeDefinition,
    refs: &HashSet<String>,
) -> bool {
    matches!(
        def.kind,
        CodeUnitKind::TraitImplMethod | CodeUnitKind::Method
    ) && refs.contains(&def.name)
        && def
            .impl_for_type
            .as_ref()
            .is_some_and(|t| refs.contains(t))
}

pub(super) fn is_covered_by_qualified_ref(
    def: &RustCodeDefinition,
    qualified_refs: &HashSet<QualifiedModuleRef>,
) -> bool {
    let def_suffix = file_to_module_suffix(&def.file);
    let stem = def
        .file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    qualified_refs.iter().any(|(module, name)| {
        if name != &def.name {
            return false;
        }
        crate_qualified_module_matches_def(&def_suffix, module)
            || (!stem.is_empty()
                && module.contains('.')
                && module.ends_with(&format!(".{stem}")))
    })
}

pub(crate) fn is_covered_by_tests(
    def: &RustCodeDefinition,
    refs: &HashSet<String>,
    qualified_refs: &HashSet<QualifiedModuleRef>,
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
) -> bool {
    is_directly_referenced(def, refs, name_files, disambiguation)
        || is_impl_method_covered_by_type_and_name(def, refs)
        || is_covered_by_qualified_ref(def, qualified_refs)
}

pub fn is_binary_entry_point(path: &Path) -> bool {
    definitions::is_binary_entry_point(path)
}

fn collect_non_test_file_refs(
    ast: &syn::File,
    test_references: &mut HashSet<String>,
    test_direct_references: &mut HashSet<String>,
    call_references: &mut HashSet<String>,
) {
    collect_test_module_references(ast, test_references);
    collect_inline_test_module_witnesses(ast, test_direct_references, call_references);
}

fn ingest_parsed_rust_file(
    parsed: &ParsedRustFile,
    definitions: &mut Vec<RustCodeDefinition>,
    test_references: &mut HashSet<String>,
    test_direct_references: &mut HashSet<String>,
    call_references: &mut HashSet<String>,
    qualified_references: &mut HashSet<QualifiedModuleRef>,
    per_test_usage: &mut PerTestUsage,
) {
    if is_rust_test_file(&parsed.path) {
        collect_rust_references(
            &parsed.ast,
            test_references,
            qualified_references,
        );
        collect_rust_references(
            &parsed.ast,
            test_direct_references,
            &mut HashSet::new(),
        );
        collect_rust_call_references(
            &parsed.ast,
            call_references,
            &mut HashSet::new(),
        );
    } else if definitions::is_binary_entry_point(&parsed.path) {
        collect_non_test_file_refs(
            &parsed.ast,
            test_references,
            test_direct_references,
            call_references,
        );
    } else {
        collect_rust_definitions(&parsed.ast, &parsed.path, definitions);
        collect_non_test_file_refs(
            &parsed.ast,
            test_references,
            test_direct_references,
            call_references,
        );
    }
    let test_funcs = collect_per_test_usage(&parsed.ast);
    if !test_funcs.is_empty() {
        per_test_usage.push((parsed.path.clone(), test_funcs));
    }
}

fn build_rust_disambiguation(
    per_test_usage: &PerTestUsage,
    name_files: &HashMap<String, HashSet<PathBuf>>,
    test_references: &HashSet<String>,
    graph: Option<&DependencyGraph>,
) -> HashMap<String, PathBuf> {
    #[allow(clippy::type_complexity)]
    let py_style_usage: Vec<(PathBuf, Vec<(String, HashSet<String>, HashSet<String>)>)> =
        per_test_usage
            .iter()
            .map(|(path, funcs)| {
                (
                    path.clone(),
                    funcs
                        .iter()
                        .map(|(id, refs)| (id.clone(), refs.clone(), HashSet::new()))
                        .collect(),
                )
            })
            .collect();
    crate::test_refs::build_disambiguation_map(name_files, test_references, &py_style_usage, graph)
}

pub fn analyze_rust_test_refs(
    parsed_files: &[&ParsedRustFile],
    graph: Option<&DependencyGraph>,
) -> RustTestRefAnalysis {
    let mut definitions = Vec::new();
    let mut test_references = HashSet::new();
    let mut test_direct_references = HashSet::new();
    let mut call_references = HashSet::new();
    let mut qualified_references = HashSet::new();
    let mut per_test_usage: PerTestUsage = Vec::new();
    for parsed in parsed_files {
        ingest_parsed_rust_file(
            parsed,
            &mut definitions,
            &mut test_references,
            &mut test_direct_references,
            &mut call_references,
            &mut qualified_references,
            &mut per_test_usage,
        );
    }
    let production_files: Vec<&ParsedRustFile> = parsed_files
        .iter()
        .copied()
        .filter(|p| {
            !is_rust_test_file(&p.path) && !definitions::is_binary_entry_point(&p.path)
        })
        .collect();
    let name_files = crate::test_refs::build_name_file_map(
        definitions
            .iter()
            .map(|d| (d.name.as_str(), d.file.as_path())),
    );
    propagate_transitive_production_refs(
        &production_files,
        &definitions,
        &name_files,
        &mut test_references,
        &mut qualified_references,
    );
    let propagated_references: HashSet<String> = test_references
        .iter()
        .filter(|name| !test_direct_references.contains(*name))
        .cloned()
        .collect();
    let disambiguation =
        build_rust_disambiguation(&per_test_usage, &name_files, &test_references, graph);
    let unreferenced = definitions
        .iter()
        .filter(|d| {
            !is_covered_by_tests(
                d,
                &test_references,
                &qualified_references,
                &name_files,
                &disambiguation,
            )
        })
        .cloned()
        .collect();
    let coverage_map = build_rust_coverage_map(
        &definitions,
        &per_test_usage,
        &name_files,
        &disambiguation,
        &qualified_references,
    );
    RustTestRefAnalysis {
        definitions,
        test_references,
        call_references,
        propagated_references,
        unreferenced,
        coverage_map,
    }
}
