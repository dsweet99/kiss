use super::coverage_expand::{is_py_base_oi_subtree, is_py_oi_interfaces_stub_path, is_py_oi_root_level_module};
use super::disambiguation::module_suffix_matches;
use super::{CodeDefinition, CoveringTest};
use crate::graph::DependencyGraph;
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

/// `base/oi/**`: credit when a test imports the module (or parent package) even if def names
/// are not referenced — capped later via `calibration_def_end_line` on base/oi paths.
/// Facade modules such as `evaluate.py` require explicit call witnesses instead.
pub(crate) fn is_py_oi_module_import_target_file(path: &std::path::Path) -> bool {
    if !is_py_base_oi_subtree(path) {
        return false;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    !matches!(name, "__init__.py" | "evaluate.py")
}

pub(crate) fn is_py_oi_module_import_witnessed(
    def: &CodeDefinition,
    import_bindings: &HashMap<String, HashSet<String>>,
    module_suffixes: &HashMap<PathBuf, String>,
) -> bool {
    if !is_py_base_oi_subtree(&def.file) || !is_py_oi_module_import_target_file(&def.file) {
        return false;
    }
    let Some(def_suffix) = module_suffixes.get(&def.file) else {
        return false;
    };
    import_bindings.keys().any(|import_module| {
        if def_suffix == import_module {
            return true;
        }
        if is_py_oi_root_level_module(&def.file)
            && !is_py_oi_interfaces_stub_path(&def.file)
        {
            return false;
        }
        module_suffix_matches(def_suffix, import_module)
    })
}

/// Package `__init__.py`: credit defs when any test imports a submodule of that package
/// (import side effects run at collection; slipcover credits those lines).
pub(crate) fn is_py_package_init_import_witnessed(
    def: &CodeDefinition,
    import_bindings: &HashMap<String, HashSet<String>>,
    module_suffixes: &HashMap<PathBuf, String>,
) -> bool {
    use super::coverage_expand::is_py_base_oi_subtree;
    use super::coverage_expand::is_py_base_subtree_only;
    if is_py_base_subtree_only(&def.file) && !is_py_base_oi_subtree(&def.file) {
        return false;
    }
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

/// `PKG/base/` (non-OI): credit when tests import the module (side effects at collection).
pub(crate) fn is_py_base_module_import_witnessed(
    def: &CodeDefinition,
    import_bindings: &HashMap<String, HashSet<String>>,
    module_suffixes: &HashMap<PathBuf, String>,
) -> bool {
    use super::coverage_expand::{is_py_base_oi_subtree, is_py_base_subtree_only};
    if !is_py_base_subtree_only(&def.file) || is_py_base_oi_subtree(&def.file) {
        return false;
    }
    let Some(def_suffix) = module_suffixes.get(&def.file) else {
        return false;
    };
    import_bindings.iter().any(|(import_module, names)| {
        def_suffix == import_module && names.is_empty()
    })
}

/// `PKG/base/` defs: credit when a test from-imports the symbol and references it in test code.
pub(crate) fn is_py_base_symbol_import_witnessed(
    def: &CodeDefinition,
    import_bindings: &HashMap<String, HashSet<String>>,
    module_suffixes: &HashMap<PathBuf, String>,
    witness_refs: &HashSet<String>,
) -> bool {
    use super::coverage_expand::is_py_base_subtree_only;
    if !is_py_base_subtree_only(&def.file) {
        return false;
    }
    let Some(def_suffix) = module_suffixes.get(&def.file) else {
        return false;
    };
    if !witness_refs.contains(&def.name) {
        return false;
    }
    import_bindings.iter().any(|(import_module, names)| {
        names.contains(&def.name) && module_suffix_matches(def_suffix, import_module)
    })
}

/// `PKG/base/` defs: credit when a test imports and calls the symbol (import-only witnesses
/// over-credit facade modules like serializer/versioning with 0% runtime).
pub(crate) fn is_py_base_explicit_import_witnessed(
    def: &CodeDefinition,
    import_bindings: &HashMap<String, HashSet<String>>,
    module_suffixes: &HashMap<PathBuf, String>,
    call_witness_refs: &HashSet<String>,
) -> bool {
    use super::coverage_expand::is_py_base_subtree_only;
    if !is_py_base_subtree_only(&def.file) {
        return false;
    }
    let Some(def_suffix) = module_suffixes.get(&def.file) else {
        return false;
    };
    let import_ok = import_bindings.iter().any(|(import_module, names)| {
        names.contains(&def.name) && module_suffix_matches(def_suffix, import_module)
    });
    if !import_ok {
        return false;
    }
    call_witness_refs.contains(&def.name)
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
    _name_files: &HashMap<String, HashSet<PathBuf>>,
) -> bool {
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

/// For `kiss-coverage-map`: credit production modules imported (transitively) from modules
/// that already have a direct test witness.
#[path = "coverage_import_cal.rs"]
mod coverage_import_cal;
#[allow(unused_imports)]
pub(crate) use coverage_import_cal::{
    apply_import_dependency_calibration, module_has_usage_witness, module_is_contrib_base_void,
};

#[path = "coverage_platform.rs"]
mod coverage_platform;
#[allow(unused_imports)]
pub(crate) use coverage_platform::{
    deprioritize_platform_gated_coverage, deprioritize_pragma_no_cover_coverage,
    is_platform_specific_prod_file, is_pragma_no_cover_def, is_windows_gated_test_file,
    platform_direct_test_witness,
};

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

