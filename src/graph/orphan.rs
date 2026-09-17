use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::comments::{normalize_allowed_dirs, path_in_allowed_dirs};
use crate::parsing::ParsedFile;
use crate::rust_include::canonical_path;
use crate::rust_parsing::ParsedRustFile;
use crate::violation::Violation;

use super::context::ContextDependencyGraph;
use super::dependency_graph::{DependencyGraph, all_module_metrics, is_orphan};
use super::graph_analyze::{
    is_init_module, is_path_covered_by_another, orphan_violation, path_dedup_set,
};

pub fn orphan_violations(
    ctx: &ContextDependencyGraph,
    prod: &DependencyGraph,
    entries: &HashSet<PathBuf>,
    orphan_allowed: &[String],
    repo_root: &Path,
) -> Vec<Violation> {
    let allowed = normalize_allowed_dirs(orphan_allowed);
    let seen_paths = path_dedup_set(prod);
    let metrics_by_module = all_module_metrics(prod);
    let entry_canon: HashSet<PathBuf> = entries.iter().map(|p| canonical_path(p)).collect();
    let mut violations = Vec::new();
    for module_name in prod.nodes.keys() {
        if !prod.paths.contains_key(module_name) {
            continue;
        }
        let Some(metrics) = metrics_by_module.get(module_name) else {
            continue;
        };
        if is_init_module(prod, module_name) {
            continue;
        }
        if !is_orphan(metrics.fan_in, metrics.fan_out, module_name) {
            continue;
        }
        if is_path_covered_by_another(prod, module_name, &seen_paths) {
            continue;
        }
        if !ctx.test_importers_of(module_name).is_empty() {
            continue;
        }
        let path = &prod.paths[module_name];
        if entry_canon.contains(&canonical_path(path)) {
            continue;
        }
        if path_in_allowed_dirs(path, repo_root, &allowed) {
            continue;
        }
        violations.push(orphan_violation(prod, module_name));
    }
    violations
}

pub fn collect_orphan_entry_paths(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
    py_graph: Option<&DependencyGraph>,
    rs_graph: Option<&DependencyGraph>,
) -> HashSet<PathBuf> {
    collect_orphan_entry_set(py_parsed, rs_parsed, py_graph, rs_graph).0
}

pub fn collect_orphan_entry_callables(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
    py_graph: Option<&DependencyGraph>,
    rs_graph: Option<&DependencyGraph>,
) -> HashSet<(PathBuf, String)> {
    collect_orphan_entry_set(py_parsed, rs_parsed, py_graph, rs_graph).1
}

fn collect_orphan_entry_set(
    py_parsed: &[ParsedFile],
    rs_parsed: &[ParsedRustFile],
    py_graph: Option<&DependencyGraph>,
    _rs_graph: Option<&DependencyGraph>,
) -> (HashSet<PathBuf>, HashSet<(PathBuf, String)>) {
    let mut entries = HashSet::new();
    let mut callables = HashSet::new();
    for parsed in py_parsed {
        if python_has_main_guard(parsed) {
            entries.insert(canonical_path(&parsed.path));
        }
    }
    if let Some(graph) = py_graph {
        for (module, callable) in python_manifest_entries(py_parsed) {
            if let Some(path) = path_for_python_module(graph, &module) {
                entries.insert(path.clone());
                if let Some(name) = callable {
                    callables.insert((path, name));
                }
            }
        }
    }
    for parsed in rs_parsed {
        if rust_has_fn_main(&parsed.ast) {
            entries.insert(canonical_path(&parsed.path));
        }
    }
    let rs_files: Vec<PathBuf> = rs_parsed.iter().map(|p| p.path.clone()).collect();
    entries.extend(crate::code_roles::cargo_entry_src_paths(&rs_files));
    (entries, callables)
}

fn python_has_main_guard(parsed: &ParsedFile) -> bool {
    walk_name_main(parsed.tree.root_node(), parsed.source.as_bytes())
}

fn walk_name_main(node: tree_sitter::Node<'_>, src: &[u8]) -> bool {
    if node.kind() == "comparison_operator" && comparison_is_name_main(node, src) {
        return true;
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| walk_name_main(child, src))
}

fn comparison_is_name_main(node: tree_sitter::Node<'_>, src: &[u8]) -> bool {
    let mut saw_name = false;
    let mut saw_main = false;
    let mut saw_eq = false;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let Ok(text) = child.utf8_text(src) else {
            continue;
        };
        match child.kind() {
            "identifier" if text == "__name__" => saw_name = true,
            "string" if unquote_py_string(text) == Some("__main__") => saw_main = true,
            _ if text == "==" => saw_eq = true,
            _ => {}
        }
    }
    saw_name && saw_main && saw_eq
}

fn unquote_py_string(text: &str) -> Option<&str> {
    let t = text.trim();
    for quote in ["\"\"\"", "'''", "\"", "'"] {
        if let Some(inner) = t.strip_prefix(quote).and_then(|s| s.strip_suffix(quote)) {
            return Some(inner);
        }
    }
    None
}

fn rust_has_fn_main(ast: &syn::File) -> bool {
    ast.items.iter().any(|item| match item {
        syn::Item::Fn(func) => func.sig.ident == "main",
        _ => false,
    })
}

fn python_manifest_entries(py_parsed: &[ParsedFile]) -> Vec<(String, Option<String>)> {
    let mut entries = Vec::new();
    for manifest in ancestor_manifests(py_parsed.iter().map(|p| p.path.as_path()), "pyproject.toml")
    {
        if let Ok(text) = std::fs::read_to_string(&manifest) {
            entries.extend(modules_from_pyproject(&text));
        }
    }
    for manifest in ancestor_manifests(py_parsed.iter().map(|p| p.path.as_path()), "setup.cfg") {
        if let Ok(text) = std::fs::read_to_string(&manifest) {
            entries.extend(modules_from_setup_cfg(&text));
        }
    }
    entries
}

fn ancestor_manifests<'a, I>(files: I, name: &str) -> HashSet<PathBuf>
where
    I: Iterator<Item = &'a Path>,
{
    let mut out = HashSet::new();
    for file in files {
        for ancestor in file.ancestors() {
            let candidate = ancestor.join(name);
            if candidate.is_file() {
                out.insert(canonical_path(&candidate));
            }
        }
    }
    out
}

fn modules_from_pyproject(text: &str) -> Vec<(String, Option<String>)> {
    let Ok(table) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    let Some(project) = table.get("project").and_then(|v| v.as_table()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_script_values(project.get("scripts"), &mut out);
    collect_script_values(project.get("gui-scripts"), &mut out);
    if let Some(entry_points) = project.get("entry-points").and_then(|v| v.as_table()) {
        for group in entry_points.values() {
            collect_script_values(Some(group), &mut out);
        }
    }
    out
}

fn collect_script_values(value: Option<&toml::Value>, out: &mut Vec<(String, Option<String>)>) {
    let Some(table) = value.and_then(|v| v.as_table()) else {
        return;
    };
    for item in table.values() {
        if let Some(raw) = item.as_str()
            && let Some(entry) = parse_script_entry(raw)
        {
            out.push(entry);
        }
    }
}

fn modules_from_setup_cfg(text: &str) -> Vec<(String, Option<String>)> {
    let mut in_section = false;
    let mut in_scripts = false;
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_section = trimmed.eq_ignore_ascii_case("[options.entry_points]");
            in_scripts = false;
            continue;
        }
        if !in_section || trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';')
        {
            continue;
        }
        let indented = line.starts_with(' ') || line.starts_with('\t');
        if !indented && trimmed.contains('=') {
            let key = trimmed.split('=').next().unwrap_or("").trim();
            in_scripts = matches!(key, "console_scripts" | "gui_scripts");
            if in_scripts
                && let Some((_, rest)) = trimmed.split_once('=')
                && let Some(entry) = parse_script_entry(rest)
            {
                out.push(entry);
            }
            continue;
        }
        if in_scripts {
            let value = trimmed.split_once('=').map_or(trimmed, |(_, rhs)| rhs);
            if let Some(entry) = parse_script_entry(value) {
                out.push(entry);
            }
        }
    }
    out
}

fn parse_script_entry(entry: &str) -> Option<(String, Option<String>)> {
    let entry = entry.trim();
    let (module, callable) = match entry.split_once(':') {
        Some((module, callable)) => {
            let name = callable.trim().rsplit('.').next().unwrap_or("").trim();
            (module.trim(), (!name.is_empty()).then(|| name.to_string()))
        }
        None => (entry, None),
    };
    if module.is_empty() {
        None
    } else {
        Some((module.to_string(), callable))
    }
}

fn path_for_python_module(graph: &DependencyGraph, module: &str) -> Option<PathBuf> {
    if let Some(path) = graph.paths.get(module) {
        return Some(canonical_path(path));
    }
    let dotted = format!(".{module}");
    let mut hits: Vec<&PathBuf> = graph
        .paths
        .iter()
        .filter(|(name, _)| *name == module || name.ends_with(&dotted))
        .map(|(_, path)| path)
        .collect();
    hits.sort();
    hits.dedup();
    if hits.len() == 1 {
        return Some(canonical_path(hits[0]));
    }
    let file_suffix = format!("{}.py", module.replace('.', "/"));
    let init_suffix = format!("{}/__init__.py", module.replace('.', "/"));
    let mut path_hits: Vec<&PathBuf> = graph
        .paths
        .values()
        .filter(|path| path.ends_with(&file_suffix) || path.ends_with(&init_suffix))
        .collect();
    path_hits.sort();
    path_hits.dedup();
    (path_hits.len() == 1).then(|| canonical_path(path_hits[0]))
}

#[cfg(test)]
#[path = "orphan_test.rs"]
mod orphan_test;
