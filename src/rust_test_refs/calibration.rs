use super::definitions::{is_binary_entry_point, RustCodeDefinition};
use super::references::{
    collect_rust_references_for_fn_coverage_map, ReferenceVisitor, RefWitnessMode,
};
use super::{has_cfg_test_attribute, has_test_attribute, is_rust_test_file};
use crate::graph::DependencyGraph;
use crate::rust_parsing::ParsedRustFile;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use syn::visit::Visit;
use syn::{ImplItem, Item, Type};

/// Max `mod` depth from binary entry when expanding integration-test execution cones.
pub(crate) const INTEGRATION_CONE_MAX_DEPTH: usize = 12;

pub(crate) use super::calibration_detect::{
    has_colocated_src_integration_tests, has_non_subprocess_integration_tests,
    has_rust_integration_test_runner, is_subprocess_integration_test_file,
};
pub(crate) use super::calibration_expand::{
    expand_small_module_defs_from_stem_refs, expand_witnessed_directory_sibling_defs,
};
pub(crate) use super::calibration_map::{
    is_calibration_excluded_file, is_coverage_map_cli_commands_file,
};

pub(crate) fn expand_coverage_map_witnesses(
    parsed_files: &[&ParsedRustFile],
    refs: &mut HashSet<String>,
) {
    if has_colocated_src_integration_tests(parsed_files) {
        expand_coverage_references_one_hop(parsed_files, refs);
        expand_small_module_defs_from_stem_refs(parsed_files, refs);
        expand_integration_cone_witnesses(parsed_files, refs);
        return;
    }
    if has_rust_integration_test_runner(parsed_files)
        && has_non_subprocess_integration_tests(parsed_files)
    {
        seed_binary_entry_roots(parsed_files, refs);
        expand_coverage_references_one_hop(parsed_files, refs);
        expand_small_module_defs_from_stem_refs(parsed_files, refs);
        expand_integration_cone_witnesses(parsed_files, refs);
    } else {
        expand_coverage_references_one_hop(parsed_files, refs);
        expand_small_module_defs_from_stem_refs(parsed_files, refs);
        expand_witnessed_directory_sibling_defs(parsed_files, refs);
    }
}

pub(crate) fn merge_one_hop_refs(
    body_refs: HashSet<String>,
    refs: &HashSet<String>,
    added: &mut HashSet<String>,
) {
    for r in body_refs {
        if !refs.contains(&r) {
            added.insert(r);
        }
    }
}

pub(crate) fn impl_self_type_name(ty: &Type) -> Option<String> {
    if let Type::Path(p) = ty {
        return p.path.segments.last().map(|s| s.ident.to_string());
    }
    None
}

pub(crate) fn integration_cone_files_for(
    parsed_files: &[&ParsedRustFile],
) -> HashSet<PathBuf> {
    if !has_rust_integration_test_runner(parsed_files)
        || !has_non_subprocess_integration_tests(parsed_files)
    {
        return HashSet::new();
    }
    let seed_paths = binary_entry_paths(parsed_files);
    if seed_paths.is_empty() {
        return HashSet::new();
    }
    integration_cone_file_paths(parsed_files, &seed_paths, INTEGRATION_CONE_MAX_DEPTH)
}

pub(crate) fn binary_entry_paths(parsed_files: &[&ParsedRustFile]) -> Vec<PathBuf> {
    parsed_files
        .iter()
        .filter(|p| is_binary_entry_point(&p.path))
        .map(|p| crate::rust_include::canonical_path(&p.path))
        .collect()
}

pub(crate) fn resolve_mod_child_path(parent_file: &Path, child: &str) -> Option<PathBuf> {
    let dir = parent_file.parent()?;
    let flat = dir.join(format!("{child}.rs"));
    if flat.is_file() {
        return Some(crate::rust_include::canonical_path(&flat));
    }
    let nested = dir.join(child).join("mod.rs");
    if nested.is_file() {
        return Some(crate::rust_include::canonical_path(&nested));
    }
    None
}

pub(crate) fn module_definition_counts(
    definitions: &[RustCodeDefinition],
    graph: &DependencyGraph,
) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for d in definitions {
        if let Some(m) = graph.path_to_module.get(&crate::rust_include::canonical_path(&d.file)) {
            *counts.entry(m.clone()).or_default() += 1;
        }
    }
    counts
}

pub(crate) fn integration_cone_file_paths(
    parsed_files: &[&ParsedRustFile],
    seed_paths: &[PathBuf],
    max_depth: usize,
) -> HashSet<PathBuf> {
    let file_by_path: HashMap<PathBuf, &ParsedRustFile> = parsed_files
        .iter()
        .map(|p| (crate::rust_include::canonical_path(&p.path), *p))
        .collect();
    let mut visited_files = HashSet::new();
    let mut frontier: Vec<(PathBuf, usize)> = seed_paths.iter().map(|p| (p.clone(), 0)).collect();
    while let Some((path, depth)) = frontier.pop() {
        if !visited_files.insert(path.clone()) {
            continue;
        }
        if depth >= max_depth {
            continue;
        }
        let Some(parsed) = file_by_path.get(&path) else {
            continue;
        };
        let imports = crate::rust_graph::extract_rust_imports(&parsed.ast);
        for child in imports.mod_decls {
            if let Some(child_path) = resolve_mod_child_path(&parsed.path, &child) {
                frontier.push((child_path, depth + 1));
            }
        }
    }
    visited_files
}

/// Witness refs reachable only via call expansion from binary `main`/`run` inside the integration cone.
#[allow(dead_code)] // exercised by unit tests; reserved for coverage-map cone diagnostics
pub(crate) fn integration_cone_witness_refs(
    parsed_files: &[&ParsedRustFile],
) -> HashSet<String> {
    let mut refs = HashSet::new();
    if !has_rust_integration_test_runner(parsed_files) {
        return refs;
    }
    seed_binary_entry_roots(parsed_files, &mut refs);
    if refs.is_empty() {
        return refs;
    }
    let seed_paths = binary_entry_paths(parsed_files);
    let cone_files =
        integration_cone_file_paths(parsed_files, &seed_paths, INTEGRATION_CONE_MAX_DEPTH);
    let cone_parsed: Vec<&ParsedRustFile> = parsed_files
        .iter()
        .copied()
        .filter(|p| cone_files.contains(&crate::rust_include::canonical_path(&p.path)))
        .collect();
    expand_coverage_references_to_fixpoint(&cone_parsed, &mut refs);
    refs
}

/// Fixpoint witness expansion limited to the `mod` tree reachable from binary entry files.
pub(crate) fn expand_integration_cone_witnesses(
    parsed_files: &[&ParsedRustFile],
    refs: &mut HashSet<String>,
) {
    if !has_rust_integration_test_runner(parsed_files) {
        return;
    }
    if !refs.contains("main") && !refs.contains("run") {
        return;
    }
    let seed_paths = binary_entry_paths(parsed_files);
    if seed_paths.is_empty() {
        return;
    }
    let cone_files =
        integration_cone_file_paths(parsed_files, &seed_paths, INTEGRATION_CONE_MAX_DEPTH);
    let cone_parsed: Vec<&ParsedRustFile> = parsed_files
        .iter()
        .copied()
        .filter(|p| cone_files.contains(&crate::rust_include::canonical_path(&p.path)))
        .collect();
    expand_coverage_references_to_fixpoint(&cone_parsed, refs);
}

pub(crate) use super::calibration_import::apply_rust_import_dependency_calibration;
#[cfg(test)]
pub(crate) use super::calibration_import::{
    module_has_rust_witness, module_is_binary_crate_src_only, module_is_rule_settings_only,
    module_is_single_crate_cli_only,
};

pub(crate) fn seed_binary_entry_roots(parsed_files: &[&ParsedRustFile], refs: &mut HashSet<String>) {
    for parsed in parsed_files {
        if is_rust_test_file(&parsed.path) || !is_binary_entry_point(&parsed.path) {
            continue;
        }
        for item in &parsed.ast.items {
            seed_binary_entry_roots_from_item(item, refs);
        }
    }
}

pub(crate) fn seed_binary_entry_roots_from_item(item: &Item, refs: &mut HashSet<String>) {
    match item {
        Item::Fn(f) if !has_test_attribute(&f.attrs) => {
            let name = f.sig.ident.to_string();
            if name == "main" || name == "run" {
                refs.insert(name);
            }
        }
        Item::Mod(m) if !has_cfg_test_attribute(&m.attrs) => {
            if let Some((_, items)) = &m.content {
                for sub in items {
                    seed_binary_entry_roots_from_item(sub, refs);
                }
            }
        }
        _ => {}
    }
}

pub(crate) fn expand_coverage_references_to_fixpoint(
    parsed_files: &[&ParsedRustFile],
    refs: &mut HashSet<String>,
) {
    const MAX_ROUNDS: usize = 64;
    for _ in 0..MAX_ROUNDS {
        let before = refs.len();
        expand_coverage_references_one_hop(parsed_files, refs);
        if refs.len() == before {
            break;
        }
    }
}

pub(crate) fn expand_coverage_references_one_hop(
    parsed_files: &[&ParsedRustFile],
    refs: &mut HashSet<String>,
) {
    let mut added = HashSet::new();
    for parsed in parsed_files {
        if is_rust_test_file(&parsed.path) {
            continue;
        }
        for item in &parsed.ast.items {
            expand_one_hop_from_item(item, refs, &mut added);
        }
    }
    refs.extend(added);
}

pub(crate) fn expand_one_hop_from_item(
    item: &Item,
    refs: &HashSet<String>,
    added: &mut HashSet<String>,
) {
    match item {
        Item::Fn(f) if !has_test_attribute(&f.attrs) => {
            let name = f.sig.ident.to_string();
            if refs.contains(&name) {
                merge_one_hop_refs(collect_rust_references_for_fn_coverage_map(f), refs, added);
            }
        }
        Item::Impl(i) if !has_cfg_test_attribute(&i.attrs) => {
            let type_name = impl_self_type_name(&i.self_ty);
            for impl_item in &i.items {
                if let ImplItem::Fn(m) = impl_item {
                    let name = m.sig.ident.to_string();
                    let type_ok = type_name.as_ref().is_none_or(|t| refs.contains(t));
                    if refs.contains(&name) && type_ok {
                        let mut body_refs = HashSet::new();
                        ReferenceVisitor {
                            refs: &mut body_refs,
                            mode: RefWitnessMode::COVERAGE_MAP,
                        }
                        .visit_block(&m.block);
                        merge_one_hop_refs(body_refs, refs, added);
                    }
                }
            }
        }
        Item::Mod(m) if !has_cfg_test_attribute(&m.attrs) => {
            if let Some((_, items)) = &m.content {
                for sub in items {
                    expand_one_hop_from_item(sub, refs, added);
                }
            }
        }
        _ => {}
    }
}

pub(crate) use super::calibration_map::build_rust_coverage_map;
