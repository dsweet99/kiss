use super::disambiguation::module_suffix_matches;
use super::{CodeDefinition, CoveringTest};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

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
