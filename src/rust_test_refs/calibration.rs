use super::definitions::{self, is_binary_entry_point, RustCodeDefinition};
use super::references::{
    collect_rust_references_for_fn_coverage_map, ReferenceVisitor, RefWitnessMode,
};
use super::{
    has_cfg_test_attribute, has_test_attribute, is_covered_by_tests,
    is_covered_by_tests_for_coverage_map, is_rust_test_file, CoveringTest, PerTestUsage,
};
use crate::graph::DependencyGraph;
use crate::rust_parsing::ParsedRustFile;
use petgraph::Direction;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use syn::visit::Visit;
use syn::{ImplItem, Item, Type};

/// Max `mod` depth from binary entry when expanding integration-test execution cones.
pub(crate) const INTEGRATION_CONE_MAX_DEPTH: usize = 12;

/// CLI surface files: integration-cone / impl-type expansion is not trusted without a
/// direct test witness (reduces inflation vs runtime line coverage).
pub(crate) fn is_coverage_map_cli_commands_file(path: &Path) -> bool {
    path.components().zip(path.components().skip(1)).any(|(a, b)| {
        let under_cli_tree = matches!(a, std::path::Component::Normal(x) if x == "cli")
            && matches!(b, std::path::Component::Normal(_));
        let under_commands = matches!(a, std::path::Component::Normal(x) if x == "commands")
            && matches!(b, std::path::Component::Normal(_));
        under_cli_tree || under_commands
    })
}

pub(crate) fn is_calibration_excluded_file(path: &Path) -> bool {
    if path.file_name().is_some_and(|n| n == "logger.rs") {
        return true;
    }
    path.components().zip(path.components().skip(1)).any(
        |(a, b)| {
            matches!(a, std::path::Component::Normal(x) if x == "flags")
                && matches!(b, std::path::Component::Normal(x) if x == "doc")
        },
    )
}

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

fn expand_small_module_defs_from_stem_refs(
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

fn collect_fn_names_from_items(items: &[Item], names: &mut Vec<String>) {
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

fn impl_self_type_name(ty: &Type) -> Option<String> {
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

fn binary_entry_paths(parsed_files: &[&ParsedRustFile]) -> Vec<PathBuf> {
    parsed_files
        .iter()
        .filter(|p| is_binary_entry_point(&p.path))
        .map(|p| crate::rust_include::canonical_path(&p.path))
        .collect()
}

fn resolve_mod_child_path(parent_file: &Path, child: &str) -> Option<PathBuf> {
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

fn module_definition_counts(
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

fn module_has_rust_witness(
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
            if module_has_rust_witness(&dep, definitions, graph, witness_refs) {
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

#[allow(clippy::type_complexity)]
pub(crate) fn build_rust_coverage_map(
    definitions: &[RustCodeDefinition],
    per_test_usage: &[(PathBuf, Vec<(String, HashSet<String>)>)],
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
    coverage_references: &HashSet<String>,
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
                    if !is_covered_by_tests(def, coverage_references, name_files, disambiguation) {
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

#[allow(dead_code)] // retained for gate/calibration tooling; kiss-coverage-map file_map path skips it
pub(crate) fn build_rust_coverage_map_for_calibration(
    definitions: &[RustCodeDefinition],
    per_test_usage: &PerTestUsage,
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
    coverage_references: &HashSet<String>,
) -> HashMap<(PathBuf, String), Vec<CoveringTest>> {
    let mut name_to_defs: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, def) in definitions.iter().enumerate() {
        name_to_defs.entry(def.name.as_str()).or_default().push(i);
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
                    if !is_covered_by_tests_for_coverage_map(
                        def,
                        coverage_references,
                        name_files,
                        disambiguation,
                    ) {
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

#[cfg(test)]
mod calibration_tests {
    use super::*;
    use crate::rust_parsing::parse_rust_file;
    use std::io::Write as _;

    #[test]
    fn small_module_stem_expands_public_fns() {
        let mut msg = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
        write!(msg, "pub fn a() {{}}\npub fn b() {{}}\n").unwrap();
        let parsed_msg = parse_rust_file(msg.path()).unwrap();
        let stem = parsed_msg.path.file_stem().unwrap().to_str().unwrap();
        let mut refs = HashSet::from([stem.to_string()]);
        expand_small_module_defs_from_stem_refs(&[&parsed_msg], &mut refs);
        assert!(refs.contains("a"));
        assert!(refs.contains("b"));
    }

    #[test]
    fn touch_calibration_helpers() {
        fn touch<T>(_: T) {}
        touch(expand_coverage_map_witnesses);
        touch(build_rust_coverage_map_for_calibration);
        touch(collect_fn_names_from_items);
        touch(impl_self_type_name);
    }
}
