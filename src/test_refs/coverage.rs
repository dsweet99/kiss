use super::disambiguation::module_suffix_matches;
use super::{CodeDefinition, CoveringTest};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

pub(crate) struct CoverageContext<'a> {
    pub name_files: &'a HashMap<String, HashSet<PathBuf>>,
    pub disambiguation: &'a HashMap<String, PathBuf>,
    pub import_bindings: &'a HashMap<String, HashSet<String>>,
    pub module_suffixes: &'a HashMap<PathBuf, String>,
    pub usage_refs: &'a HashSet<String>,
    pub call_refs: &'a HashSet<String>,
    pub alias_bindings: &'a HashMap<String, String>,
}

pub(crate) fn is_method_covered_by_class_and_name(
    def: &CodeDefinition,
    usage_refs: &HashSet<String>,
) -> bool {
    def.containing_class
        .as_ref()
        .is_some_and(|cls| usage_refs.contains(cls) && usage_refs.contains(&def.name))
}

pub(crate) fn is_definition_covered(def: &CodeDefinition, ctx: &CoverageContext<'_>) -> bool {
    if is_method_covered_by_class_and_name(def, ctx.usage_refs) {
        return true;
    }
    if ctx.usage_refs.contains(&def.name) {
        let unique = ctx.name_files.get(&def.name).is_none_or(|f| f.len() <= 1);
        if unique {
            return true;
        }
        if let Some(winner) = ctx.disambiguation.get(&def.name)
            && *winner == def.file
        {
            return true;
        }
    }
    is_import_called(
        def,
        ctx.import_bindings,
        ctx.module_suffixes,
        ctx.call_refs,
        ctx.alias_bindings,
    )
}

fn is_import_called(
    def: &CodeDefinition,
    import_bindings: &HashMap<String, HashSet<String>>,
    module_suffixes: &HashMap<PathBuf, String>,
    call_refs: &HashSet<String>,
    alias_bindings: &HashMap<String, String>,
) -> bool {
    let Some(def_suffix) = module_suffixes.get(&def.file) else {
        return false;
    };
    import_bindings.iter().any(|(import_module, names)| {
        if !names.contains(&def.name) || !module_suffix_matches(def_suffix, import_module) {
            return false;
        }
        if call_refs.contains(&def.name) {
            return true;
        }
        alias_bindings
            .iter()
            .any(|(alias, original)| original == &def.name && call_refs.contains(alias))
    })
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
    }

    ref_to_defs
}

#[allow(clippy::type_complexity)]
pub(crate) fn build_py_coverage_map(
    definitions: &[CodeDefinition],
    per_test_usage: &[(PathBuf, Vec<(String, HashSet<String>, HashSet<String>)>)],
    name_files: &HashMap<String, HashSet<PathBuf>>,
    disambiguation: &HashMap<String, PathBuf>,
    import_bindings: &HashMap<String, HashSet<String>>,
    module_suffixes: &HashMap<PathBuf, String>,
    alias_bindings: &HashMap<String, String>,
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
        for (test_id, _usage_refs, call_refs) in test_funcs {
            let ti = test_idx;
            test_entries.push((test_path.clone(), test_id.clone()));
            test_idx += 1;
            let mut seen = HashSet::new();
            for ref_name in call_refs {
                let resolved = alias_bindings
                    .get(ref_name)
                    .map_or(ref_name.as_str(), String::as_str);
                let Some(def_indices) = ref_to_defs.get(resolved) else {
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
