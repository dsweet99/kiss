use crate::test_refs::coverage_expand::is_py_base_oi_subtree;
use crate::test_refs::{CodeDefinition, TestRefAnalysis};
use crate::graph::DependencyGraph;
use petgraph::Direction;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// For `kiss-coverage-map`: credit production modules imported (transitively) from modules
/// that already have a direct test witness.
const MAX_IMPORT_CALIBRATION_DEFS_PER_MODULE: usize = 2;
const MAX_IMPORT_CALIBRATION_DEFS_BASE_OI: usize = 48;

pub(crate) fn import_calibration_def_cap(
    module: &str,
    definitions: &[CodeDefinition],
    graph: &DependencyGraph,
) -> usize {
    let oi = definitions.iter().any(|d| {
        graph.path_to_module.get(&d.file).is_some_and(|m| m == module)
            && is_py_base_oi_subtree(&d.file)
    });
    if oi {
        MAX_IMPORT_CALIBRATION_DEFS_BASE_OI
    } else {
        MAX_IMPORT_CALIBRATION_DEFS_PER_MODULE
    }
}

pub(crate) fn module_is_contrib_base_void(
    module: &str,
    definitions: &[CodeDefinition],
    graph: &DependencyGraph,
) -> bool {
    use crate::test_refs::coverage_expand::is_py_contrib_base_void_partition;
    definitions.iter().any(|d| {
        graph.path_to_module.get(&d.file).is_some_and(|m| m == module)
            && is_py_contrib_base_void_partition(&d.file)
            && !is_py_base_oi_subtree(&d.file)
    })
}

pub(crate) fn module_has_usage_witness(
    module: &str,
    definitions: &[CodeDefinition],
    graph: &DependencyGraph,
    usage_refs: &HashSet<String>,
    name_files: &HashMap<String, HashSet<PathBuf>>,
) -> bool {
    definitions.iter().any(|d| {
        graph.path_to_module.get(&d.file).is_some_and(|m| m == module)
            && usage_refs.contains(&d.name)
            && name_files.get(&d.name).is_none_or(|files| files.len() <= 1)
    })
}

pub(crate) fn import_cal_dep_qualifies(
    dep: &str,
    definitions: &[CodeDefinition],
    graph: &DependencyGraph,
    usage_refs: &HashSet<String>,
    name_files: &HashMap<String, HashSet<PathBuf>>,
) -> bool {
    module_has_usage_witness(dep, definitions, graph, usage_refs, name_files)
}

pub(crate) fn apply_import_dependency_calibration(
    analysis: &mut TestRefAnalysis,
    graph: &DependencyGraph,
    usage_refs: &HashSet<String>,
    name_files: &HashMap<String, HashSet<PathBuf>>,
) {
    use super::coverage_platform::is_platform_specific_prod_file;
    let defs_per_module = super::module_definition_counts(&analysis.definitions, graph);
    let unref_keys: HashSet<(PathBuf, String, usize)> = analysis
        .unreferenced
        .iter()
        .map(|d| (d.file.clone(), d.name.clone(), d.line))
        .collect();
    let mut covered_modules: HashSet<String> = analysis
        .definitions
        .iter()
        .filter(|d| !unref_keys.contains(&(d.file.clone(), d.name.clone(), d.line)))
        .filter_map(|d| graph.path_to_module.get(&d.file).cloned())
        .collect();
    let seeds: Vec<String> = covered_modules.iter().cloned().collect();
    for mod_name in seeds {
        let Some(&idx) = graph.nodes.get(&mod_name) else {
            continue;
        };
        for neighbor in graph
            .graph
            .neighbors_directed(idx, Direction::Outgoing)
        {
            let dep = graph.graph[neighbor].clone();
            if defs_per_module
                .get(&dep)
                .copied()
                .unwrap_or(usize::MAX)
                > import_calibration_def_cap(&dep, &analysis.definitions, graph)
            {
                continue;
            }
            if import_cal_dep_qualifies(
                &dep,
                &analysis.definitions,
                graph,
                usage_refs,
                name_files,
            ) && !module_is_contrib_base_void(&dep, &analysis.definitions, graph)
            {
                covered_modules.insert(dep);
            }
        }
    }
    analysis.unreferenced.retain(|d| {
        if is_platform_specific_prod_file(&d.file) {
            return true;
        }
        use crate::test_refs::coverage_expand::{
            is_py_base_oi_subtree, is_py_contrib_base_void_partition, is_py_inflator_calibration_path,
        };
        let key = (d.file.clone(), d.name.clone(), d.line);
        // Filter-tier OI/module credit must not be undone when path_to_module is absent (g11).
        if is_py_base_oi_subtree(&d.file) && !unref_keys.contains(&key) {
            return false;
        }
        if (is_py_contrib_base_void_partition(&d.file) && !is_py_base_oi_subtree(&d.file))
            || is_py_inflator_calibration_path(&d.file)
        {
            return unref_keys.contains(&key);
        }
        graph
            .path_to_module
            .get(&d.file)
            .is_none_or(|m| !covered_modules.contains(m))
    });
}