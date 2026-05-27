use super::definitions::{self, is_binary_entry_point, RustCodeDefinition};
use super::references::{
    collect_rust_references_for_fn_coverage_map, ReferenceVisitor, RefWitnessMode,
};
use super::{has_cfg_test_attribute, has_test_attribute, is_rust_test_file};
use crate::graph::DependencyGraph;
use crate::rust_parsing::ParsedRustFile;
use petgraph::Direction;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use syn::visit::Visit;
use syn::{ImplItem, Item, Type};

/// Max `mod` depth from binary entry when expanding integration-test execution cones.
pub(crate) const INTEGRATION_CONE_MAX_DEPTH: usize = 12;

pub(crate) use super::calibration_map::{
    is_calibration_excluded_file, is_coverage_map_cli_commands_file,
};

pub(crate) fn expand_coverage_map_witnesses(
    parsed_files: &[&ParsedRustFile],
    refs: &mut HashSet<String>,
) {
    if has_rust_integration_test_runner(parsed_files) {
        seed_binary_entry_roots(parsed_files, refs);
        expand_coverage_references_one_hop(parsed_files, refs);
        expand_small_module_defs_from_stem_refs(parsed_files, refs);
        expand_integration_cone_witnesses(parsed_files, refs);
    } else {
        expand_coverage_references_one_hop(parsed_files, refs);
    }
}

const MAX_MODULE_STEM_EXPAND_DEFS: usize = 8;

pub(crate) fn expand_small_module_defs_from_stem_refs(
    parsed_files: &[&ParsedRustFile],
    refs: &mut HashSet<String>,
) {
    for parsed in parsed_files {
        if is_rust_test_file(&parsed.path) {
            continue;
        }
        let Some(stem) = parsed.path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if !refs.contains(stem) {
            continue;
        }
        let mut names = Vec::new();
        collect_fn_names_from_items(&parsed.ast.items, &mut names);
        if names.len() > MAX_MODULE_STEM_EXPAND_DEFS {
            continue;
        }
        for name in names {
            refs.insert(name);
        }
    }
}

pub(crate) fn collect_fn_names_from_items(items: &[Item], names: &mut Vec<String>) {
    for item in items {
        match item {
            Item::Fn(f)
                if !has_test_attribute(&f.attrs)
                    && !definitions::is_private(&f.sig.ident.to_string()) =>
            {
                names.push(f.sig.ident.to_string());
            }
            Item::Mod(m) if !has_cfg_test_attribute(&m.attrs) => {
                if let Some((_, sub)) = &m.content {
                    collect_fn_names_from_items(sub, names);
                }
            }
            _ => {}
        }
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
    if !has_rust_integration_test_runner(parsed_files) {
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

const MAX_IMPORT_CALIBRATION_DEFS_PER_MODULE: usize = 12;

pub(crate) fn module_has_rust_witness(
    module: &str,
    definitions: &[RustCodeDefinition],
    graph: &DependencyGraph,
    witness_refs: &HashSet<String>,
) -> bool {
    definitions.iter().any(|d| {
        graph
            .path_to_module
            .get(&crate::rust_include::canonical_path(&d.file))
            .is_some_and(|m| m == module)
            && witness_refs.contains(&d.name)
    })
}

/// Credit small dependency modules when a neighboring module already has a direct test witness.
pub(crate) fn apply_rust_import_dependency_calibration(
    definitions: &[RustCodeDefinition],
    unreferenced: &mut Vec<RustCodeDefinition>,
    graph: &DependencyGraph,
    witness_refs: &HashSet<String>,
) {
    let defs_per_module = module_definition_counts(definitions, graph);
    let unref_keys: HashSet<(&PathBuf, &str, usize)> = unreferenced
        .iter()
        .map(|d| (&d.file, d.name.as_str(), d.line))
        .collect();
    let mut covered_modules: HashSet<String> = definitions
        .iter()
        .filter(|d| !unref_keys.contains(&(&d.file, d.name.as_str(), d.line)))
        .filter_map(|d| {
            graph
                .path_to_module
                .get(&crate::rust_include::canonical_path(&d.file))
                .cloned()
        })
        .collect();
    let seeds: Vec<String> = covered_modules.iter().cloned().collect();
    for mod_name in seeds {
        let Some(&idx) = graph.nodes.get(&mod_name) else {
            continue;
        };
        for neighbor in graph.graph.neighbors_directed(idx, Direction::Outgoing) {
            let dep = graph.graph[neighbor].clone();
            if defs_per_module
                .get(&dep)
                .copied()
                .unwrap_or(usize::MAX)
                > MAX_IMPORT_CALIBRATION_DEFS_PER_MODULE
            {
                continue;
            }
            if module_has_rust_witness(&mod_name, definitions, graph, witness_refs) {
                covered_modules.insert(dep);
            }
        }
    }
    unreferenced.retain(|d| {
        if is_calibration_excluded_file(&d.file) {
            return true;
        }
        let key = crate::rust_include::canonical_path(&d.file);
        graph
            .path_to_module
            .get(&key)
            .is_none_or(|m| !covered_modules.contains(m))
    });
}

pub(crate) fn has_rust_integration_test_runner(parsed_files: &[&ParsedRustFile]) -> bool {
    parsed_files.iter().any(|parsed| {
        path_is_under_tests(&parsed.path)
            && (parsed.source.contains("current_exe")
                || parsed.source.contains("Command::new"))
    })
}

pub(crate) fn path_is_under_tests(path: &Path) -> bool {
    path.components()
        .any(|c| matches!(c, std::path::Component::Normal(s) if s == "tests"))
}

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
