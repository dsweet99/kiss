use super::disambiguation::module_suffix_matches;
use super::{CodeDefinition, CoveringTest, TestRefAnalysis};
use crate::graph::DependencyGraph;
use petgraph::Direction;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

fn import_bindings_match_package(
    pkg: &str,
    import_bindings: &HashMap<String, HashSet<String>>,
) -> bool {
    import_bindings
        .keys()
        .any(|k| module_suffix_matches(pkg, k))
}

fn init_suffix_matches_imports(
    def_suffix: &str,
    import_bindings: &HashMap<String, HashSet<String>>,
) -> bool {
    def_suffix
        .strip_suffix(".__init__")
        .is_some_and(|pkg| import_bindings_match_package(pkg, import_bindings))
}

fn init_stem_matches_imports(
    stem: &str,
    import_bindings: &HashMap<String, HashSet<String>>,
) -> bool {
    import_bindings
        .keys()
        .any(|k| k == stem || k.starts_with(&format!("{stem}.")))
}

/// Package `__init__.py`: credit defs when any test imports a submodule of that package
/// (import side effects run at collection; slipcover credits those lines).
pub(crate) fn is_py_package_init_import_witnessed(
    def: &CodeDefinition,
    import_bindings: &HashMap<String, HashSet<String>>,
    module_suffixes: &HashMap<PathBuf, String>,
) -> bool {
    if def.file.file_name().and_then(|n| n.to_str()) != Some("__init__.py") {
        return false;
    }
    let parent_stem = def
        .file
        .parent()
        .and_then(|p| p.file_stem())
        .and_then(|s| s.to_str());
    let suffix_match = module_suffixes
        .get(&def.file)
        .is_some_and(|def_suffix| init_suffix_matches_imports(def_suffix, import_bindings));
    let stem_match = parent_stem.is_some_and(|stem| init_stem_matches_imports(stem, import_bindings));
    suffix_match || stem_match
}

pub(crate) fn is_covered_by_import(
    def: &CodeDefinition,
    import_bindings: &HashMap<String, HashSet<String>>,
    module_suffixes: &HashMap<PathBuf, String>,
    usage_refs: &HashSet<String>,
) -> bool {
    import_matches_definition(def, import_bindings, module_suffixes, usage_refs)
}

pub(crate) fn is_covered_by_import_for_calibration(
    def: &CodeDefinition,
    import_bindings: &HashMap<String, HashSet<String>>,
    module_suffixes: &HashMap<PathBuf, String>,
    usage_refs: &HashSet<String>,
    name_files: &HashMap<String, HashSet<PathBuf>>,
) -> bool {
    if name_files.get(&def.name).is_some_and(|files| files.len() > 1) {
        return false;
    }
    import_matches_definition(def, import_bindings, module_suffixes, usage_refs)
}

pub(crate) fn import_matches_definition(
    def: &CodeDefinition,
    import_bindings: &HashMap<String, HashSet<String>>,
    module_suffixes: &HashMap<PathBuf, String>,
    usage_refs: &HashSet<String>,
) -> bool {
    if !usage_refs.contains(&def.name) {
        return false;
    }
    let Some(def_suffix) = module_suffixes.get(&def.file) else {
        return false;
    };
    import_bindings.iter().any(|(import_module, names)| {
        names.contains(&def.name) && module_suffix_matches(def_suffix, import_module)
    })
}

pub(crate) fn is_definition_covered(
    def: &CodeDefinition,
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
    import_bindings: &HashMap<String, HashSet<String>>,
    module_suffixes: &HashMap<PathBuf, String>,
    usage_refs: &HashSet<String>,
) -> bool {
    if is_covered_by_import(def, import_bindings, module_suffixes, usage_refs) {
        return true;
    }
    if usage_refs.contains(&def.name) {
        let unique = name_files.get(&def.name).is_none_or(|f| f.len() <= 1);
        if unique {
            return true;
        }
        if let Some(winner) = disambiguation.get(&def.name)
            && *winner == def.file
        {
            return true;
        }
    }
    if let Some(ref cls) = def.containing_class {
        return usage_refs.contains(cls);
    }
    false
}

/// Like [`is_definition_covered`], but does not treat a class name in `usage_refs` as covering
/// every method on that class (reduces inflation vs runtime line coverage).
pub(crate) fn is_definition_covered_for_calibration(
    def: &CodeDefinition,
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
    import_bindings: &HashMap<String, HashSet<String>>,
    module_suffixes: &HashMap<PathBuf, String>,
    usage_refs: &HashSet<String>,
) -> bool {
    if is_covered_by_import_for_calibration(
        def,
        import_bindings,
        module_suffixes,
        usage_refs,
        name_files,
    ) {
        return true;
    }
    if usage_refs.contains(&def.name) {
        let unique = name_files.get(&def.name).is_none_or(|f| f.len() <= 1);
        if unique {
            return true;
        }
        if let Some(winner) = disambiguation.get(&def.name)
            && *winner == def.file
        {
            return true;
        }
        if def
            .file
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|stem| stem == def.name)
            && name_files.get(&def.name).is_none_or(|files| files.len() <= 1)
        {
            return true;
        }
    }
    false
}

/// For `kiss-coverage-map`: credit production modules imported (transitively) from modules
/// that already have a direct test witness.
const MAX_IMPORT_CALIBRATION_DEFS_PER_MODULE: usize = 2;

#[path = "coverage_platform.rs"]
mod coverage_platform;
#[allow(unused_imports)]
pub(crate) use coverage_platform::{
    deprioritize_platform_gated_coverage, deprioritize_pragma_no_cover_coverage,
    is_platform_specific_prod_file, is_pragma_no_cover_def, is_windows_gated_test_file,
    platform_direct_test_witness,
};

pub(crate) fn module_is_contrib_base_void(
    module: &str,
    definitions: &[CodeDefinition],
    graph: &DependencyGraph,
) -> bool {
    use super::coverage_expand::is_py_contrib_base_void_partition;
    definitions.iter().any(|d| {
        graph.path_to_module.get(&d.file).is_some_and(|m| m == module)
            && is_py_contrib_base_void_partition(&d.file)
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

pub(crate) fn apply_import_dependency_calibration(
    analysis: &mut TestRefAnalysis,
    graph: &DependencyGraph,
    usage_refs: &HashSet<String>,
    name_files: &HashMap<String, HashSet<PathBuf>>,
) {
    let defs_per_module = module_definition_counts(&analysis.definitions, graph);
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
                > MAX_IMPORT_CALIBRATION_DEFS_PER_MODULE
            {
                continue;
            }
            if module_has_usage_witness(&dep, &analysis.definitions, graph, usage_refs, name_files)
                && !module_is_contrib_base_void(&dep, &analysis.definitions, graph)
            {
                covered_modules.insert(dep);
            }
        }
    }
    analysis.unreferenced.retain(|d| {
        if is_platform_specific_prod_file(&d.file) {
            return true;
        }
        use super::coverage_expand::{is_py_contrib_base_void_partition, is_py_optimizer_experiment_path};
        if is_py_contrib_base_void_partition(&d.file) || is_py_optimizer_experiment_path(&d.file) {
            return unref_keys.contains(&(d.file.clone(), d.name.clone(), d.line));
        }
        graph
            .path_to_module
            .get(&d.file)
            .is_none_or(|m| !covered_modules.contains(m))
    });
}

pub(crate) fn module_definition_counts(
    definitions: &[CodeDefinition],
    graph: &DependencyGraph,
) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for d in definitions {
        if let Some(m) = graph.path_to_module.get(&d.file) {
            *counts.entry(m.clone()).or_default() += 1;
        }
    }
    counts
}

pub(crate) fn build_ref_to_covered_def_indices(
    definitions: &[CodeDefinition],
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
    import_bindings: &HashMap<String, HashSet<String>>,
    module_suffixes: &HashMap<PathBuf, String>,
) -> HashMap<String, Vec<usize>> {
    let mut ref_to_defs: HashMap<String, Vec<usize>> = HashMap::new();

    for (i, def) in definitions.iter().enumerate() {
        let unique = name_files.get(&def.name).is_none_or(|f| f.len() <= 1);
        let disambiguated = disambiguation
            .get(&def.name)
            .is_some_and(|w| *w == def.file);
        let import_matched = module_suffixes.get(&def.file).is_some_and(|def_suffix| {
            import_bindings.iter().any(|(import_module, names)| {
                names.contains(&def.name) && module_suffix_matches(def_suffix, import_module)
            })
        });

        if unique || disambiguated || import_matched {
            ref_to_defs.entry(def.name.clone()).or_default().push(i);
        }

        if let Some(ref cls) = def.containing_class {
            ref_to_defs.entry(cls.clone()).or_default().push(i);
        }
    }

    ref_to_defs
}

#[allow(clippy::type_complexity)]
pub(crate) fn build_py_coverage_map(
    definitions: &[CodeDefinition],
    per_test_usage: &[(PathBuf, Vec<(String, HashSet<String>)>)],
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
    import_bindings: &HashMap<String, HashSet<String>>,
    module_suffixes: &HashMap<PathBuf, String>,
) -> HashMap<(PathBuf, String), Vec<CoveringTest>> {
    let ref_to_defs = build_ref_to_covered_def_indices(
        definitions,
        name_files,
        disambiguation,
        import_bindings,
        module_suffixes,
    );

    let mut idx_map: HashMap<usize, Vec<usize>> = HashMap::new();

    let mut test_entries: Vec<(PathBuf, String)> = Vec::new();
    let mut test_idx = 0usize;
    for (test_path, test_funcs) in per_test_usage {
        for (test_id, usage_refs) in test_funcs {
            let ti = test_idx;
            test_entries.push((test_path.clone(), test_id.clone()));
            test_idx += 1;
            let mut seen = HashSet::new();
            for ref_name in usage_refs {
                let Some(def_indices) = ref_to_defs.get(ref_name) else {
                    continue;
                };
                for &di in def_indices {
                    if !seen.insert(di) {
                        continue;
                    }
                    idx_map.entry(di).or_default().push(ti);
                }
            }
        }
    }

    idx_map
        .into_iter()
        .map(|(di, test_indices)| {
            let def = &definitions[di];
            let key = (def.file.clone(), def.name.clone());
            let tests: Vec<CoveringTest> = test_indices
                .into_iter()
                .map(|ti| test_entries[ti].clone())
                .collect();
            (key, tests)
        })
        .collect()
}

