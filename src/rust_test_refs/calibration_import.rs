use super::calibration_map::{
    is_calibration_excluded_file, is_coverage_map_binary_crate_src_root,
    is_coverage_map_rule_settings_file, is_coverage_map_single_crate_cli_file,
};
use super::coverage_map_unreferenced::{
    coverage_map_forced_uncovered_file, is_coverage_map_integration_cone_inflation_shim,
};
use super::definitions::RustCodeDefinition;
use super::calibration::module_definition_counts;
use crate::graph::DependencyGraph;
use petgraph::Direction;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

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

fn import_cal_vetoed_file(path: &Path) -> bool {
    coverage_map_forced_uncovered_file(path)
        || is_coverage_map_integration_cone_inflation_shim(path)
        || is_calibration_excluded_file(path)
        || is_coverage_map_binary_crate_src_root(path)
}

fn import_cal_allowed_lines(
    module: &str,
    definitions: &[RustCodeDefinition],
    graph: &DependencyGraph,
) -> HashSet<usize> {
    let mut lines: Vec<usize> = definitions
        .iter()
        .filter(|other| {
            graph
                .path_to_module
                .get(&crate::rust_include::canonical_path(&other.file))
                == Some(&module.to_string())
        })
        .map(|other| other.line)
        .collect();
    lines.sort_unstable();
    lines
        .into_iter()
        .take(MAX_IMPORT_CALIBRATION_DEFS_PER_MODULE)
        .collect()
}

fn import_cal_stay_unreferenced(
    d: &RustCodeDefinition,
    graph: &DependencyGraph,
    covered_modules: &HashSet<String>,
    defs_per_module: &std::collections::HashMap<String, usize>,
    allowed_lines: &std::collections::HashMap<String, HashSet<usize>>,
) -> bool {
    if import_cal_vetoed_file(&d.file) {
        return true;
    }
    let key = crate::rust_include::canonical_path(&d.file);
    let Some(module) = graph.path_to_module.get(&key) else {
        return true;
    };
    if !covered_modules.contains(module) {
        return true;
    }
    let module_defs = defs_per_module.get(module).copied().unwrap_or(0);
    if module_defs <= MAX_IMPORT_CALIBRATION_DEFS_PER_MODULE {
        return false;
    }
    let lines = allowed_lines
        .get(module)
        .expect("allowed lines cached for capped import-cal modules");
    !lines.contains(&d.line)
}

fn expand_import_cal_covered_modules(
    definitions: &[RustCodeDefinition],
    graph: &DependencyGraph,
    witness_refs: &HashSet<String>,
    covered_modules: &mut HashSet<String>,
) {
    let seeds: Vec<String> = covered_modules.iter().cloned().collect();
    for mod_name in seeds {
        let Some(&idx) = graph.nodes.get(&mod_name) else {
            continue;
        };
        for neighbor in graph.graph.neighbors_directed(idx, Direction::Outgoing) {
            let dep = graph.graph[neighbor].clone();
            if module_has_rust_witness(&mod_name, definitions, graph, witness_refs)
                && !module_is_binary_crate_src_only(&dep, definitions, graph)
                && !module_is_single_crate_cli_only(&dep, definitions, graph)
                && !module_is_rule_settings_only(&dep, definitions, graph)
            {
                covered_modules.insert(dep);
            }
        }
    }
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
    expand_import_cal_covered_modules(definitions, graph, witness_refs, &mut covered_modules);
    let mut allowed_lines = std::collections::HashMap::new();
    for module in &covered_modules {
        if defs_per_module.get(module).copied().unwrap_or(0) > MAX_IMPORT_CALIBRATION_DEFS_PER_MODULE
        {
            allowed_lines.insert(module.clone(), import_cal_allowed_lines(module, definitions, graph));
        }
    }
    unreferenced.retain(|d| {
        import_cal_stay_unreferenced(d, graph, &covered_modules, &defs_per_module, &allowed_lines)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::DependencyGraph;
    use crate::units::CodeUnitKind;

    #[test]
    fn import_cal_helper_paths() {
        assert!(import_cal_vetoed_file(&PathBuf::from(
            "src/output/stdout_tee_env.rs"
        )));
        assert!(!import_cal_vetoed_file(&PathBuf::from("src/acme/neighbor.rs")));

        let mut graph = DependencyGraph::new();
        graph
            .path_to_module
            .insert(PathBuf::from("src/acme/neighbor.rs"), "acme_neighbor".into());
        let mut definitions = Vec::new();
        for i in 0..14 {
            definitions.push(RustCodeDefinition {
                name: format!("f{i}"),
                kind: CodeUnitKind::Function,
                file: PathBuf::from("src/acme/neighbor.rs"),
                line: i + 1,
                end_line: i + 1,
                impl_for_type: None,
            });
        }
        let allowed = import_cal_allowed_lines("acme_neighbor", &definitions, &graph);
        assert_eq!(allowed.len(), MAX_IMPORT_CALIBRATION_DEFS_PER_MODULE);

        let mut covered = HashSet::from(["acme_neighbor".to_string()]);
        let defs_per_module =
            std::collections::HashMap::from([("acme_neighbor".to_string(), 14usize)]);
        let mut allowed_map = std::collections::HashMap::new();
        allowed_map.insert("acme_neighbor".to_string(), allowed);
        let early = &definitions[0];
        let late = &definitions[13];
        assert!(!import_cal_stay_unreferenced(
            early,
            &graph,
            &covered,
            &defs_per_module,
            &allowed_map
        ));
        assert!(import_cal_stay_unreferenced(
            late,
            &graph,
            &covered,
            &defs_per_module,
            &allowed_map
        ));

        let witness_path = PathBuf::from("src/acme/witness.rs");
        graph
            .path_to_module
            .insert(witness_path.clone(), "acme_witness".into());
        graph.get_or_create_node("acme_witness");
        graph.get_or_create_node("acme_neighbor");
        graph.add_dependency("acme_witness", "acme_neighbor");
        definitions.push(RustCodeDefinition {
            name: "seed".into(),
            kind: CodeUnitKind::Function,
            file: witness_path,
            line: 1,
            end_line: 1,
            impl_for_type: None,
        });
        let witness = HashSet::from(["seed".to_string()]);
        expand_import_cal_covered_modules(&definitions, &graph, &witness, &mut covered);
        assert!(covered.contains("acme_neighbor"));
    }
}
