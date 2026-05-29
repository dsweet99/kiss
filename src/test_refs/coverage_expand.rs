use super::collect::{
    collect_py_import_names_for_refs, collect_usage_refs_in_scope, is_dunder_main_guard,
};
use super::detection::is_python_test_file;
use super::disambiguation::file_to_module_suffix;
use super::CodeDefinition;
use crate::parsing::ParsedFile;
use crate::units::get_child_by_field;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use tree_sitter::Node;

#[path = "coverage_expand_paths.rs"]
mod coverage_expand_paths;
#[path = "coverage_void_dispatch.rs"]
mod coverage_void_dispatch;

pub(crate) use coverage_expand_paths::{
    expand_py_path_literal_file_witnesses,
    is_py_contrib_base_void_partition, is_py_contrib_refactor_void_force_uncovered,
    is_py_base_oi_subtree, is_py_base_subtree_only, is_py_ecosystem_auxiliary_path, is_py_experiments_path, is_py_inflator_calibration_path, is_py_inflator_call_only_path,
    is_py_optimizer_path,
    is_py_rl_integration_path,
};
pub use coverage_expand_paths::calibration_def_end_line;
pub use coverage_expand_paths::is_py_inflator_denominator_path;
#[allow(unused_imports)]
pub(crate) use coverage_void_dispatch::{
    build_py_void_dispatch_attestation, collect_py_dynamic_dispatch_literals,
    collect_py_raises_dotted_paths, void_files_for_dotted_path,
};

const MAX_EXPAND_HOPS: usize = 12;
pub(crate) const MAX_PRODUCTION_IMPORT_EXPAND_DEFS: usize = 12;
pub(crate) const MAX_SAME_FILE_ONE_HOP_DEFS: usize = 4;
pub(crate) const MAX_DIR_SIBLING_EXPAND_DEFS: usize = 8;

pub(crate) fn expand_py_usage_refs_fixpoint(
    parsed_files: &[&ParsedFile],
    refs: &mut HashSet<String>,
) {
    for _ in 0..MAX_EXPAND_HOPS {
        let added = one_hop_py_refs(parsed_files, refs);
        if added.is_empty() {
            break;
        }
        refs.extend(added);
    }
}

/// When any module stem in a directory is witnessed, credit defs in small sibling modules
/// (mirrors Rust `expand_witnessed_directory_sibling_defs` for facade/protocol files).
pub(crate) fn expand_py_witnessed_directory_sibling_defs(
    definitions: &[CodeDefinition],
    refs: &mut HashSet<String>,
) {
    let mut by_dir: BTreeMap<PathBuf, Vec<&CodeDefinition>> = BTreeMap::new();
    for def in definitions {
        if is_python_test_file_path(&def.file) {
            continue;
        }
        let Some(parent) = def.file.parent() else {
            continue;
        };
        by_dir
            .entry(parent.to_path_buf())
            .or_default()
            .push(def);
    }
    for defs in by_dir.values() {
        let dir_has_stem_witness = defs.iter().any(|d| {
            d.file
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|stem| refs.contains(stem))
        });
        if !dir_has_stem_witness {
            continue;
        }
        let mut names_by_file: BTreeMap<&PathBuf, Vec<&str>> = BTreeMap::new();
        for d in defs.iter() {
            names_by_file.entry(&d.file).or_default().push(d.name.as_str());
        }
        for (file, names) in names_by_file {
            let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if refs.contains(stem) {
                continue;
            }
            if stem.starts_with('_') {
                continue;
            }
            if is_py_contrib_base_void_partition(file) && !is_py_base_oi_subtree(file) {
                continue;
            }
            if names.len() > MAX_DIR_SIBLING_EXPAND_DEFS {
                continue;
            }
            for name in names {
                refs.insert(name.to_string());
            }
        }
    }
}

/// `base/oi/**/interfaces.py`: credit protocol stubs when a sibling module in the same directory
/// already has a witnessed def (implementor-downward, not submodule blanket).
pub(crate) fn expand_py_oi_interface_downward_witnesses(
    definitions: &[CodeDefinition],
    refs: &mut HashSet<String>,
) {
    let mut by_dir: BTreeMap<PathBuf, Vec<&CodeDefinition>> = BTreeMap::new();
    for def in definitions {
        if is_python_test_file_path(&def.file) || !is_py_base_oi_subtree(&def.file) {
            continue;
        }
        let Some(parent) = def.file.parent() else {
            continue;
        };
        by_dir
            .entry(parent.to_path_buf())
            .or_default()
            .push(def);
    }
    for defs in by_dir.values() {
        let dir_has_concrete_witness = defs.iter().any(|d| {
            d.file
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n != "interfaces.py")
                && refs.contains(&d.name)
        });
        if !dir_has_concrete_witness {
            continue;
        }
        for d in defs.iter().filter(|d| {
            d.file
                .file_name()
                .and_then(|n| n.to_str())
                == Some("interfaces.py")
        }) {
            if defs
                .iter()
                .filter(|x| x.file == d.file)
                .count()
                > MAX_DIR_SIBLING_EXPAND_DEFS
            {
                continue;
            }
            refs.insert(d.name.clone());
        }
    }
}

pub(crate) fn is_python_test_file_path(path: &std::path::Path) -> bool {
    path.components().any(|c| {
        matches!(
            c,
            std::path::Component::Normal(s) if s.to_str().is_some_and(|n| n == "tests" || n == "test")
        )
    })
}

pub(crate) fn expand_py_same_file_one_hop(
    parsed_files: &[&ParsedFile],
    definitions: &[CodeDefinition],
    refs: &mut HashSet<String>,
) {
    for parsed in parsed_files {
        if is_python_test_file(parsed) {
            continue;
        }
        let file_defs: Vec<&CodeDefinition> = definitions
            .iter()
            .filter(|d| d.file == parsed.path)
            .collect();
        if file_defs.len() > MAX_PRODUCTION_IMPORT_EXPAND_DEFS {
            continue;
        }
        let has_witness = file_defs.iter().any(|d| refs.contains(&d.name));
        if !has_witness {
            continue;
        }
        let added = one_hop_py_refs(std::slice::from_ref(parsed), refs);
        refs.extend(added);
    }
}

/// When any name from a production `from … import a, b` is witnessed, add sibling imports.
/// When a production file with a witnessed symbol imports names, add those imports to refs
/// (reduces blind spots vs runtime line coverage for small re-export modules).
pub(crate) fn expand_py_refs_via_production_imports(
    parsed_files: &[&ParsedFile],
    definitions: &[CodeDefinition],
    refs: &mut HashSet<String>,
) {
    for parsed in parsed_files {
        if is_python_test_file(parsed) || is_py_contrib_base_void_partition(&parsed.path) {
            continue;
        }
        let file_defs: Vec<&CodeDefinition> = definitions
            .iter()
            .filter(|d| d.file == parsed.path)
            .collect();
        if file_defs.len() > MAX_PRODUCTION_IMPORT_EXPAND_DEFS {
            continue;
        }
        let has_witness = file_defs.iter().any(|d| refs.contains(&d.name));
        if !has_witness {
            continue;
        }
        collect_all_production_import_names(parsed.tree.root_node(), &parsed.source, refs);
    }
}

fn collect_all_production_import_names(node: Node, source: &str, refs: &mut HashSet<String>) {
    if node.kind() == "if_statement" && is_dunder_main_guard(node, source) {
        return;
    }
    if node.kind() == "import_from_statement" {
        refs.extend(import_names_from_statement(node, source));
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_all_production_import_names(child, source, refs);
    }
}

pub(crate) fn expand_py_import_sibling_refs(
    parsed_files: &[&ParsedFile],
    refs: &mut HashSet<String>,
) {
    for parsed in parsed_files {
        if is_python_test_file(parsed) {
            continue;
        }
        let suffix = file_to_module_suffix(&parsed.path);
        collect_import_siblings(parsed.tree.root_node(), &parsed.source, &suffix, refs);
    }
}

pub(crate) fn resolve_relative_module_suffix(
    importer_suffix: &str,
    relative: &str,
) -> Option<String> {
    let dot_count = relative.chars().take_while(|c| *c == '.').count();
    let module_name = relative[dot_count..].trim();
    if module_name.is_empty() {
        return None;
    }
    let mut segments: Vec<&str> = importer_suffix.split('.').collect();
    if segments.is_empty() {
        return None;
    }
    segments.pop();
    for _ in 0..dot_count.saturating_sub(1) {
        segments.pop();
    }
    segments.push(module_name);
    Some(segments.join("."))
}

pub(crate) fn collect_import_siblings(
    node: Node,
    source: &str,
    importer_suffix: &str,
    refs: &mut HashSet<String>,
) {
    if node.kind() == "import_from_statement" {
        merge_import_sibling_names(node, source, importer_suffix, refs);
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_import_siblings(child, source, importer_suffix, refs);
    }
}

pub(crate) fn merge_import_sibling_names(
    node: Node,
    source: &str,
    importer_suffix: &str,
    refs: &mut HashSet<String>,
) {
    let Some(module_node) = node.child_by_field_name("module_name") else {
        return;
    };
    let module_path = &source[module_node.start_byte()..module_node.end_byte()];
    let resolved = if module_path.starts_with('.') {
        resolve_relative_module_suffix(importer_suffix, module_path)
    } else {
        Some(module_path.to_string())
    };
    let Some(_target) = resolved else {
        return;
    };
    let names = import_names_from_statement(node, source);
    if !names.iter().any(|n| refs.contains(n)) {
        return;
    }
    refs.extend(names);
}

pub(crate) fn import_names_from_statement(node: Node, source: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    collect_py_import_names_for_refs(node, source, &mut names);
    names
}

pub(crate) fn one_hop_py_refs(parsed_files: &[&ParsedFile], refs: &HashSet<String>) -> HashSet<String> {
    let mut added = HashSet::new();
    for parsed in parsed_files {
        if is_python_test_file(parsed) {
            continue;
        }
        collect_one_hop_from_node(
            parsed.tree.root_node(),
            &parsed.source,
            refs,
            &mut added,
            None,
        );
    }
    added
}

pub(crate) fn collect_one_hop_from_node(
    node: Node,
    source: &str,
    refs: &HashSet<String>,
    added: &mut HashSet<String>,
    class_name: Option<&str>,
) {
    if node.kind() == "if_statement" && is_dunder_main_guard(node, source) {
        return;
    }
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            let fname = get_child_by_field(node, "name", source).unwrap_or_default();
            let expand = refs.contains(&fname)
                || class_name.is_some_and(|c| refs.contains(c));
            if expand {
                merge_py_body_refs(node, source, refs, added);
            }
        }
        "class_definition" => {
            let cname = get_child_by_field(node, "name", source);
            if let Some(body) = node.child_by_field_name("body") {
                let mut cursor = body.walk();
                for child in body.children(&mut cursor) {
                    collect_one_hop_from_node(child, source, refs, added, cname.as_deref());
                }
            }
            return;
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_one_hop_from_node(child, source, refs, added, class_name);
    }
}

pub(crate) fn merge_py_body_refs(
    node: Node,
    source: &str,
    refs: &HashSet<String>,
    added: &mut HashSet<String>,
) {
    let Some(body) = node.child_by_field_name("body") else {
        return;
    };
    let mut body_refs = HashSet::new();
    collect_usage_refs_in_scope(body, source, &mut body_refs);
    for r in body_refs {
        if !refs.contains(&r) {
            added.insert(r);
        }
    }
}

#[cfg(test)]
#[path = "tests_coverage_expand.rs"]
mod tests_coverage_expand;
#[cfg(test)]
#[path = "tests_coverage_expand_extra.rs"]
mod tests_coverage_expand_extra;
#[cfg(test)]
#[path = "tests_coverage_expand_extra_b.rs"]
mod tests_coverage_expand_extra_b;
