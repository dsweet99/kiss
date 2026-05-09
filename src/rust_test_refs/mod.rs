use crate::graph::DependencyGraph;
use crate::rust_parsing::ParsedRustFile;
use crate::units::CodeUnitKind;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use syn::Attribute;

mod definitions;
mod references;

#[cfg(test)]
mod tests_1;
#[cfg(test)]
mod tests_2;

pub use definitions::RustCodeDefinition;
use definitions::{collect_rust_definitions, collect_test_module_references};
use references::{collect_per_test_usage, collect_rust_references};

use crate::test_refs::CoveringTest;

type PerTestUsage = Vec<(PathBuf, Vec<(String, HashSet<String>)>)>;

#[derive(Debug, Clone)]
pub struct RustTestRefAnalysis {
    pub definitions: Vec<RustCodeDefinition>,
    pub test_references: HashSet<String>,
    pub unreferenced: Vec<RustCodeDefinition>,
    /// For each covered definition (file, name), the list of tests that reference it.
    pub coverage_map: HashMap<(PathBuf, String), Vec<CoveringTest>>,
}

fn is_rs_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
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
    is_directly_referenced(def, refs, name_files, disambiguation)
        || is_impl_with_referenced_type(def, refs)
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
    let coverage_map =
        build_rust_coverage_map(&definitions, &per_test_usage, &name_files, &disambiguation);
    RustTestRefAnalysis {
        definitions,
        test_references,
        unreferenced,
        coverage_map,
    }
}

#[allow(clippy::type_complexity)]
fn build_rust_coverage_map(
    definitions: &[RustCodeDefinition],
    per_test_usage: &[(PathBuf, Vec<(String, HashSet<String>)>)],
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
) -> HashMap<(PathBuf, String), Vec<CoveringTest>> {
    let mut name_to_defs: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, def) in definitions.iter().enumerate() {
        name_to_defs.entry(&def.name).or_default().push(i);
        if let Some(ref t) = def.impl_for_type {
            name_to_defs.entry(t.as_str()).or_default().push(i);
        }
    }

    let mut coverage_map: HashMap<(PathBuf, String), Vec<CoveringTest>> = HashMap::new();
    for (test_path, test_funcs) in per_test_usage {
        for (test_id, usage_refs) in test_funcs {
            if test_id.is_empty() {
                continue;
            }
            let mut seen = HashSet::new();
            for ref_name in usage_refs {
                let Some(def_indices) = name_to_defs.get(ref_name.as_str()) else {
                    continue;
                };
                for &idx in def_indices {
                    if !seen.insert(idx) {
                        continue;
                    }
                    let def = &definitions[idx];
                    if !is_covered_by_tests(def, usage_refs, name_files, disambiguation) {
                        continue;
                    }
                    let key = (def.file.clone(), def.name.clone());
                    let entry = (test_path.clone(), test_id.clone());
                    let list = coverage_map.entry(key).or_default();
                    if !list.contains(&entry) {
                        list.push(entry);
                    }
                }
            }
        }
    }
    coverage_map
}
