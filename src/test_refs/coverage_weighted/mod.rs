mod map_mode;

pub use map_mode::py_init_marker_pct;

use super::scope::count_py_branches;
use super::{CodeDefinition, TestRefAnalysis};
use crate::parsing::ParsedFile;
use crate::py_metrics::compute_function_metrics;
use crate::units::{CodeUnitKind, get_child_by_field};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tree_sitter::Node;

fn py_call_witness(analysis: &TestRefAnalysis, file: &Path, name: &str) -> bool {
    analysis
        .coverage_map
        .contains_key(&(file.to_path_buf(), name.to_string()))
}

fn find_def_node_at_line<'a>(root: Node<'a>, line: usize) -> Option<Node<'a>> {
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "class_definition" && child.start_position().row + 1 == line {
            return Some(child);
        }
    }
    find_function_at_line(root, line)
}

fn class_covering_tests<'a>(
    analysis: &'a TestRefAnalysis,
    unref_set: &HashSet<(&PathBuf, &str)>,
    file: &PathBuf,
    class_name: &str,
) -> Option<&'a [(PathBuf, String)]> {
    if unref_set.contains(&(file, class_name)) {
        return None;
    }
    analysis
        .coverage_map
        .get(&(file.clone(), class_name.to_string()))
        .map(Vec::as_slice)
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
                        if matches!(
                            method.kind(),
                            "function_definition" | "async_function_definition"
                        ) && method.start_position().row + 1 == line
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
                    if matches!(
                        method.kind(),
                        "function_definition" | "async_function_definition"
                    ) && get_child_by_field(method, "name", source).as_deref() == Some(fn_name)
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
        if matches!(
            child.kind(),
            "function_definition" | "async_function_definition"
        ) && get_child_by_field(child, "name", source).as_deref() == Some(test_id)
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
    count_py_branches(node)
}

fn module_import_surface_credit(
    def: &CodeDefinition,
    analysis: &TestRefAnalysis,
    node: Node,
    source: &str,
    covering: &[(PathBuf, String)],
    parsed_by_path: &HashMap<PathBuf, &ParsedFile>,
) -> Option<f64> {
    if def.containing_class.is_some() || def.kind == CodeUnitKind::Class {
        return None;
    }
    let sibling_count = analysis
        .definitions
        .iter()
        .filter(|d| {
            d.file == def.file && d.containing_class.is_none() && d.kind != CodeUnitKind::Class
        })
        .count()
        .max(1);
    let metrics = compute_function_metrics(node, source);
    let def_mass = metrics.statements.max(1) as f64;
    let b_ref = covering
        .iter()
        .filter_map(|(path, test_id)| parsed_by_path.get(path).map(|p| (p, test_id.as_str())))
        .map(|(p, test_id)| test_function_branches(p, test_id))
        .min()
        .unwrap_or(0);
    if b_ref == 0 {
        return Some(0.0);
    }
    Some((b_ref as f64 / (sibling_count as f64 * def_mass)).min(1.0))
}

fn class_import_surface_credit(
    def: &CodeDefinition,
    analysis: &TestRefAnalysis,
    node: Node,
    source: &str,
    covering: &[(PathBuf, String)],
    parsed_by_path: &HashMap<PathBuf, &ParsedFile>,
) -> Option<f64> {
    let class_name = def.containing_class.as_deref()?;
    let method_count = analysis
        .definitions
        .iter()
        .filter(|d| d.file == def.file && d.containing_class.as_deref() == Some(class_name))
        .count()
        .max(1);
    let metrics = compute_function_metrics(node, source);
    let def_branches = metrics.branches.max(1);
    let b_ref = covering
        .iter()
        .filter_map(|(path, test_id)| parsed_by_path.get(path).map(|p| (p, test_id.as_str())))
        .map(|(p, test_id)| test_function_branches(p, test_id))
        .min()
        .unwrap_or(0);
    if b_ref == 0 {
        return Some(0.0);
    }
    Some((b_ref as f64 / (method_count as f64 * def_branches as f64)).min(1.0))
}

fn definition_branch_credit(
    def: &CodeDefinition,
    analysis: &TestRefAnalysis,
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
        return if py_call_witness(analysis, &def.file, &def.name) {
            1.0
        } else {
            0.0
        };
    }
    let b_ref = covering_tests
        .iter()
        .filter_map(|(path, test_id)| parsed_by_path.get(path).map(|p| (p, test_id.as_str())))
        .map(|(p, test_id)| test_function_branches(p, test_id))
        .min()
        .unwrap_or(0);
    if b_ref == 0 {
        return 0.0;
    }
    if metrics.branches <= b_ref {
        1.0
    } else {
        b_ref as f64 / metrics.branches as f64
    }
}

fn py_function_weighted_credit(
    def: &CodeDefinition,
    analysis: &TestRefAnalysis,
    parsed: &ParsedFile,
    node: tree_sitter::Node,
    unref_set: &HashSet<(&PathBuf, &str)>,
    parsed_by_path: &HashMap<PathBuf, &ParsedFile>,
) -> f64 {
    if unref_set.contains(&(&def.file, def.name.as_str())) {
        if def.containing_class.is_some()
            && py_call_witness(analysis, &def.file, &def.name)
            && let Some(cls) = def.containing_class.as_ref()
            && let Some(covering) = class_covering_tests(analysis, unref_set, &def.file, cls)
        {
            return class_import_surface_credit(
                def,
                analysis,
                node,
                &parsed.source,
                covering,
                parsed_by_path,
            )
            .unwrap_or(0.0);
        }
        return 0.0;
    }
    let covering = analysis
        .coverage_map
        .get(&(def.file.clone(), def.name.clone()))
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if def.containing_class.is_none()
        && def.kind != CodeUnitKind::Class
        && !py_call_witness(analysis, &def.file, &def.name)
    {
        return module_import_surface_credit(
            def,
            analysis,
            node,
            &parsed.source,
            covering,
            parsed_by_path,
        )
        .unwrap_or_else(|| {
            definition_branch_credit(def, analysis, parsed, covering, parsed_by_path)
        });
    }
    definition_branch_credit(def, analysis, parsed, covering, parsed_by_path)
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
        let (stmts, credit) = if def.kind == CodeUnitKind::Class {
            let class_credit = if unref_set.contains(&(&def.file, def.name.as_str())) {
                0.0
            } else if py_call_witness(analysis, &def.file, &def.name) {
                1.0
            } else {
                0.0
            };
            (1_usize, class_credit)
        } else {
            let Some(node) = find_def_node_at_line(root, def.line) else {
                continue;
            };
            let stmts = compute_function_metrics(node, &parsed.source)
                .statements
                .max(1);
            let credit = py_function_weighted_credit(
                def,
                analysis,
                parsed,
                node,
                &unref_set,
                &parsed_by_path,
            );
            (stmts, credit)
        };
        let entry = by_file.entry(def.file.clone()).or_default();
        entry.0 += stmts as f64 * credit;
        entry.1 += stmts as f64;
    }

    let mut result: HashMap<PathBuf, usize> = HashMap::new();
    for (file, (covered_mass, total_mass)) in by_file {
        let pct = if total_mass > 0.0 {
            ((covered_mass / total_mass) * 100.0).round() as usize
        } else {
            0
        };
        result.insert(file, pct);
    }
    result
}

#[cfg(test)]
mod branch_tests;
#[cfg(test)]
mod inline_tests;
