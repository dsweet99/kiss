use super::calibration_map::{
    is_calibration_excluded_file, is_coverage_map_binary_crate_src_root,
    is_coverage_map_rule_settings_file, is_coverage_map_single_crate_cli_file,
};
use super::definitions::RustCodeDefinition;
use super::calibration::module_definition_counts;
use crate::graph::DependencyGraph;
use petgraph::Direction;
use std::collections::{HashSet};
use std::path::PathBuf;

const MAX_IMPORT_CALIBRATION_DEFS_PER_MODULE: usize = 12;

pub(crate) fn module_is_binary_crate_src_only(
    module: &str,
    definitions: &[RustCodeDefinition],
    graph: &DependencyGraph,
) -> bool {
    let mut any = false;
    for d in definitions {
        let key = crate::rust_include::canonical_path(&d.file);
        if graph.path_to_module.get(&key).is_some_and(|m| m == module) {
            any = true;
            if !is_coverage_map_binary_crate_src_root(&d.file) {
                return false;
            }
        }
    }
    any
}

pub(crate) fn module_is_single_crate_cli_only(
    module: &str,
    definitions: &[RustCodeDefinition],
    graph: &DependencyGraph,
) -> bool {
    let mut any = false;
    for d in definitions {
        let key = crate::rust_include::canonical_path(&d.file);
        if graph.path_to_module.get(&key).is_some_and(|m| m == module) {
            any = true;
            if !is_coverage_map_single_crate_cli_file(&d.file) {
                return false;
            }
        }
    }
    any
}

pub(crate) fn module_is_rule_settings_only(
    module: &str,
    definitions: &[RustCodeDefinition],
    graph: &DependencyGraph,
) -> bool {
    let mut any = false;
    for d in definitions {
        let key = crate::rust_include::canonical_path(&d.file);
        if graph.path_to_module.get(&key).is_some_and(|m| m == module) {
            any = true;
            if !is_coverage_map_rule_settings_file(&d.file) {
                return false;
            }
        }
    }
    any
}

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
            if module_has_rust_witness(&mod_name, definitions, graph, witness_refs)
                && !module_is_binary_crate_src_only(&dep, definitions, graph)
                && !module_is_single_crate_cli_only(&dep, definitions, graph)
                && !module_is_rule_settings_only(&dep, definitions, graph)
            {
                covered_modules.insert(dep);
            }
        }
    }
    unreferenced.retain(|d| {
        if is_calibration_excluded_file(&d.file) {
            return true;
        }
        if is_coverage_map_binary_crate_src_root(&d.file) {
            return true;
        }
        let key = crate::rust_include::canonical_path(&d.file);
        graph
            .path_to_module
            .get(&key)
            .is_none_or(|m| !covered_modules.contains(m))
    });
}
