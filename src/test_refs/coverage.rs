use super::disambiguation::module_suffix_matches;
use super::{CodeDefinition, CoveringTest, TestRefAnalysis};
use crate::parsing::ParsedFile;
use crate::py_metrics::compute_function_metrics;
use crate::units::get_child_by_field;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use tree_sitter::Node;

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

pub(crate) fn is_method_covered_by_class_and_name(
    def: &CodeDefinition,
    usage_refs: &HashSet<String>,
) -> bool {
    def.containing_class.as_ref().is_some_and(|cls| {
        usage_refs.contains(cls) && usage_refs.contains(&def.name)
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
    if is_method_covered_by_class_and_name(def, usage_refs) {
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

fn find_function_at_line<'a>(root: Node<'a>, line: usize) -> Option<Node<'a>> {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        match child.kind() {
            "function_definition" | "async_function_definition"
                if child.start_position().row + 1 == line =>
            {
                return Some(child);
            }
            "class_definition" => {
                if let Some(body) = child.child_by_field_name("body") {
                    let mut bc = body.walk();
                    for method in body.children(&mut bc) {
                        if matches!(method.kind(), "function_definition" | "async_function_definition")
                            && method.start_position().row + 1 == line
                        {
                            return Some(method);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn find_test_function_node<'a>(root: Node<'a>, source: &str, test_id: &str) -> Option<Node<'a>> {
    if let Some((class_prefix, fn_name)) = test_id.rsplit_once("::") {
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            if child.kind() != "class_definition" {
                continue;
            }
            let Some(name) = get_child_by_field(child, "name", source) else {
                continue;
            };
            if name != class_prefix {
                continue;
            }
            if let Some(body) = child.child_by_field_name("body") {
                let mut bc = body.walk();
                for method in body.children(&mut bc) {
                    if matches!(method.kind(), "function_definition" | "async_function_definition")
                        && get_child_by_field(method, "name", source).as_deref() == Some(fn_name)
                    {
                        return Some(method);
                    }
                }
            }
        }
        return None;
    }
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if matches!(child.kind(), "function_definition" | "async_function_definition")
            && get_child_by_field(child, "name", source).as_deref() == Some(test_id)
        {
            return Some(child);
        }
    }
    None
}

fn test_function_branches(parsed: &ParsedFile, test_id: &str) -> usize {
    let root = parsed.tree.root_node();
    let Some(node) = find_test_function_node(root, &parsed.source, test_id) else {
        return 0;
    };
    compute_function_metrics(node, &parsed.source).branches
}

fn definition_branch_credit(
    def: &CodeDefinition,
    parsed: &ParsedFile,
    covering_tests: &[(PathBuf, String)],
    parsed_by_path: &HashMap<PathBuf, &ParsedFile>,
) -> f64 {
    let root = parsed.tree.root_node();
    let Some(node) = find_function_at_line(root, def.line) else {
        return 1.0;
    };
    let metrics = compute_function_metrics(node, &parsed.source);
    if metrics.branches == 0 {
        return 1.0;
    }
    let b_ref = covering_tests
        .iter()
        .filter_map(|(path, test_id)| parsed_by_path.get(path).map(|p| (p, test_id.as_str())))
        .map(|(p, test_id)| test_function_branches(p, test_id))
        .max()
        .unwrap_or(0);
    if metrics.branches <= b_ref {
        1.0
    } else {
        b_ref as f64 / metrics.branches as f64
    }
}

pub fn compute_py_weighted_file_pcts(
    analysis: &TestRefAnalysis,
    parsed_files: &[&ParsedFile],
) -> HashMap<PathBuf, usize> {
    let parsed_by_path: HashMap<PathBuf, &ParsedFile> =
        parsed_files.iter().map(|p| (p.path.clone(), *p)).collect();
    let unref_set: HashSet<(&PathBuf, &str)> = analysis
        .unreferenced
        .iter()
        .map(|d| (&d.file, d.name.as_str()))
        .collect();

    let mut by_file: HashMap<PathBuf, (f64, f64)> = HashMap::new();
    for def in &analysis.definitions {
        let Some(parsed) = parsed_by_path.get(&def.file) else {
            continue;
        };
        let root = parsed.tree.root_node();
        let Some(node) = find_function_at_line(root, def.line) else {
            continue;
        };
        let stmts = compute_function_metrics(node, &parsed.source).statements.max(1);
        let credit = if unref_set.contains(&(&def.file, def.name.as_str())) {
            0.0
        } else {
            let covering = analysis
                .coverage_map
                .get(&(def.file.clone(), def.name.clone()))
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            definition_branch_credit(def, parsed, covering, &parsed_by_path)
        };
        let entry = by_file.entry(def.file.clone()).or_default();
        entry.0 += stmts as f64 * credit;
        entry.1 += stmts as f64;
    }

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    by_file
        .into_iter()
        .map(|(file, (covered_mass, total_mass))| {
            let pct = if total_mass > 0.0 {
                ((covered_mass / total_mass) * 100.0).round() as usize
            } else {
                100
            };
            (file, pct)
        })
        .collect()
}
