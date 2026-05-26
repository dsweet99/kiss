use super::disambiguation::module_suffix_matches;
use super::{CodeDefinition, CoveringTest, PerTestUsage, TestRefAnalysis};
use crate::graph::DependencyGraph;
use petgraph::Direction;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub(crate) fn is_covered_by_import(
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
    false
}

/// For `kiss-coverage-map`: credit production modules imported (transitively) from modules
/// that already have a direct test witness.
const MAX_IMPORT_CALIBRATION_DEFS_PER_MODULE: usize = 12;

pub(crate) fn is_platform_specific_prod_file(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s.contains("_win32") || s.contains("_windows") || s.contains("_extension")
}

pub(crate) fn is_windows_gated_test_file(source: &str) -> bool {
    source.contains("platform != \"win32\"")
        || source.contains("platform != 'win32'")
        || source.contains("sys.platform != \"win32\"")
        || source.contains("sys.platform != 'win32'")
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn deprioritize_platform_gated_coverage(
    definitions: &[CodeDefinition],
    unreferenced: &mut Vec<CodeDefinition>,
    per_test_usage: &PerTestUsage,
    parsed_files: &[&crate::parsing::ParsedFile],
    _name_files: &HashMap<String, HashSet<PathBuf>>,
    _disambiguation: &HashMap<String, PathBuf>,
    _import_bindings: &HashMap<String, HashSet<String>>,
    _module_suffixes: &HashMap<PathBuf, String>,
) {
    let gated_tests: HashSet<&Path> = parsed_files
        .iter()
        .filter(|p| is_windows_gated_test_file(&p.source))
        .map(|p| p.path.as_path())
        .collect();
    if gated_tests.is_empty() {
        return;
    }
    let mut to_add = Vec::new();
    for def in definitions {
        if !is_platform_specific_prod_file(&def.file) {
            continue;
        }
        let direct_test_witness =
            platform_direct_test_witness(def, per_test_usage, &gated_tests);
        let already_unref = unreferenced
            .iter()
            .any(|u| u.file == def.file && u.name == def.name && u.line == def.line);
        if already_unref {
            if direct_test_witness {
                unreferenced.retain(|u| {
                    u.file != def.file || u.name != def.name || u.line != def.line
                });
            }
            continue;
        }
        if direct_test_witness {
            continue;
        }
        to_add.push(def.clone());
    }
    unreferenced.extend(to_add);
}

fn platform_direct_test_witness(
    def: &CodeDefinition,
    per_test_usage: &PerTestUsage,
    gated_tests: &HashSet<&Path>,
) -> bool {
    let gated_names: HashSet<&str> = per_test_usage
        .iter()
        .filter(|(path, _)| gated_tests.contains(path.as_path()))
        .flat_map(|(_, funcs)| funcs.iter().flat_map(|(_, refs)| refs.iter().map(String::as_str)))
        .collect();
    for (test_path, funcs) in per_test_usage {
        if gated_tests.contains(test_path.as_path()) {
            continue;
        }
        for (_, refs) in funcs {
            if refs.contains(&def.name) {
                return true;
            }
            if def.containing_class.as_ref().is_some_and(|c| refs.contains(c))
                && !gated_names.contains(def.name.as_str())
            {
                return true;
            }
        }
    }
    false
}

fn module_has_usage_witness(
    module: &str,
    definitions: &[CodeDefinition],
    graph: &DependencyGraph,
    usage_refs: &HashSet<String>,
) -> bool {
    definitions.iter().any(|d| {
        graph.path_to_module.get(&d.file).is_some_and(|m| m == module)
            && usage_refs.contains(&d.name)
    })
}

pub(crate) fn apply_import_dependency_calibration(
    analysis: &mut TestRefAnalysis,
    graph: &DependencyGraph,
    usage_refs: &HashSet<String>,
) {
    let defs_per_module = module_definition_counts(&analysis.definitions, graph);
    let unref_keys: HashSet<(&PathBuf, &str, usize)> = analysis
        .unreferenced
        .iter()
        .map(|d| (&d.file, d.name.as_str(), d.line))
        .collect();
    let mut covered_modules: HashSet<String> = analysis
        .definitions
        .iter()
        .filter(|d| !unref_keys.contains(&(&d.file, d.name.as_str(), d.line)))
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
            if module_has_usage_witness(&dep, &analysis.definitions, graph, usage_refs) {
                covered_modules.insert(dep);
            }
        }
    }
    analysis.unreferenced.retain(|d| {
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

#[cfg(test)]
mod coverage_unit_tests {
    use super::*;
    use crate::graph::DependencyGraph;
    use crate::units::CodeUnitKind;
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    #[test]
    fn platform_direct_test_witness_paths() {
        let gated: HashSet<&Path> = HashSet::from([Path::new("tests/gated_test.py")]);
        let def = CodeDefinition {
            name: "api".into(),
            kind: CodeUnitKind::Method,
            file: PathBuf::from("rich/_windows.py"),
            line: 1,
            end_line: 2,
            containing_class: Some("Win".into()),
        };
        let per_test: PerTestUsage = vec![
            (
                PathBuf::from("tests/gated_test.py"),
                vec![("test_gated".into(), HashSet::from(["api".into()]))],
            ),
            (
                PathBuf::from("tests/test_ok.py"),
                vec![("test_ok".into(), HashSet::from(["Win".into()]))],
            ),
        ];
        assert!(!platform_direct_test_witness(&def, &per_test, &gated));
        let per_test_direct: PerTestUsage = vec![(
            PathBuf::from("tests/test_ok.py"),
            vec![("test_ok".into(), HashSet::from(["api".into()]))],
        )];
        assert!(platform_direct_test_witness(&def, &per_test_direct, &gated));
    }

    #[test]
    fn module_has_usage_witness_paths() {
        let mut graph = DependencyGraph::new();
        let path = PathBuf::from("/proj/helper.py");
        graph
            .path_to_module
            .insert(path.clone(), "helper".to_string());
        let definitions = vec![CodeDefinition {
            name: "helper_only".into(),
            kind: CodeUnitKind::Function,
            file: path,
            line: 1,
            end_line: 2,
            containing_class: None,
        }];
        let empty: HashSet<String> = HashSet::new();
        assert!(!module_has_usage_witness(
            "helper",
            &definitions,
            &graph,
            &empty
        ));
        let mut usage = HashSet::new();
        usage.insert("helper_only".into());
        assert!(module_has_usage_witness(
            "helper",
            &definitions,
            &graph,
            &usage
        ));
    }
}
