//! Python-specific metrics computation

use crate::parsing::ParsedFile;
use std::collections::HashSet;
use tree_sitter::Node;

#[derive(Debug, Default)]
pub struct FunctionMetrics {
    pub statements: usize, pub arguments: usize, pub arguments_positional: usize, pub arguments_keyword_only: usize,
    pub max_indentation: usize, pub nested_function_depth: usize, pub returns: usize, pub branches: usize, pub local_variables: usize,
    pub max_try_block_statements: usize,
}

#[derive(Debug, Default)]
pub struct ClassMetrics { pub methods: usize, pub lcom: f64 }

#[derive(Debug, Default)]
pub struct FileMetrics { pub lines: usize, pub classes: usize, pub imports: usize }

#[must_use]
pub fn compute_function_metrics(node: Node, source: &str) -> FunctionMetrics {
    let mut m = FunctionMetrics::default();
    if let Some(params) = node.child_by_field_name("parameters") {
        let c = count_parameters(params);
        m.arguments = c.total; m.arguments_positional = c.positional; m.arguments_keyword_only = c.keyword_only;
    }
    if let Some(body) = node.child_by_field_name("body") {
        m.statements = count_statements(body);
        m.max_indentation = compute_max_indentation(body, 0);
        m.branches = count_branches(body);
        m.local_variables = count_local_variables(body, source);
        m.returns = count_node_kind(body, "return_statement");
        m.max_try_block_statements = compute_max_try_block_statements(body);
    }
    m.nested_function_depth = compute_nested_function_depth(node, 0);
    m
}

#[must_use]
pub fn compute_class_metrics(node: Node) -> ClassMetrics {
    let Some(body) = node.child_by_field_name("body") else { return ClassMetrics::default() };
    ClassMetrics { methods: count_node_kind(body, "function_definition"), lcom: 0.0 }
}

#[must_use]
pub fn compute_class_metrics_with_source(node: Node, source: &str) -> ClassMetrics {
    let Some(body) = node.child_by_field_name("body") else { return ClassMetrics::default() };
    ClassMetrics { methods: count_node_kind(body, "function_definition"), lcom: compute_lcom(body, source) }
}

#[must_use]
pub fn compute_file_metrics(parsed: &ParsedFile) -> FileMetrics {
    let root = parsed.tree.root_node();
    FileMetrics { lines: parsed.source.lines().count(), classes: count_node_kind(root, "class_definition"), imports: count_imports(root) }
}

struct ParameterCounts { positional: usize, keyword_only: usize, total: usize }

fn count_parameters(params: Node) -> ParameterCounts {
    let (mut positional, mut keyword_only, mut after_star) = (0, 0, false);
    let mut cursor = params.walk();
    for child in params.children(&mut cursor) {
        match child.kind() {
            "identifier" | "default_parameter" | "typed_parameter" | "typed_default_parameter" => if after_star { keyword_only += 1 } else { positional += 1 },
            "list_splat_pattern" | "dictionary_splat_pattern" | "*" | "keyword_separator" => after_star = true,
            _ => {}
        }
    }
    ParameterCounts { positional, keyword_only, total: positional + keyword_only }
}

fn count_statements(node: Node) -> usize {
    let mut cursor = node.walk();
    node.children(&mut cursor).map(|c| usize::from(is_statement(c.kind())) + count_statements(c)).sum()
}

fn is_statement(kind: &str) -> bool {
    matches!(kind, "expression_statement" | "return_statement" | "pass_statement" | "break_statement" | "continue_statement" 
        | "raise_statement" | "assert_statement" | "global_statement" | "nonlocal_statement" | "import_statement" 
        | "import_from_statement" | "future_import_statement" | "if_statement" | "for_statement" | "while_statement" 
        | "try_statement" | "with_statement" | "match_statement" | "function_definition" | "class_definition" 
        | "decorated_definition" | "async_function_definition" | "async_for_statement" | "async_with_statement" 
        | "delete_statement" | "exec_statement" | "print_statement" | "type_alias_statement")
}

fn compute_max_indentation(node: Node, current_depth: usize) -> usize {
    let depth_inc = matches!(node.kind(), "if_statement" | "for_statement" | "while_statement" | "try_statement" 
        | "with_statement" | "match_statement" | "function_definition" | "class_definition" | "async_function_definition" 
        | "async_for_statement" | "async_with_statement" | "elif_clause" | "else_clause" | "except_clause" | "finally_clause" | "case_clause");
    let new_depth = if depth_inc { current_depth + 1 } else { current_depth };
    let mut cursor = node.walk();
    node.children(&mut cursor).fold(new_depth, |max, c| max.max(compute_max_indentation(c, new_depth)))
}

fn count_branches(node: Node) -> usize {
    let mut cursor = node.walk();
    node.children(&mut cursor).map(|c| usize::from(matches!(c.kind(), "if_statement" | "elif_clause")) + count_branches(c)).sum()
}

fn compute_max_try_block_statements(node: Node) -> usize {
    let mut max = 0;
    if node.kind() == "try_statement" {
        // The try block body is the first "block" child of the try_statement
        if let Some(body) = node.child_by_field_name("body") {
            max = max.max(count_statements(body));
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        max = max.max(compute_max_try_block_statements(child));
    }
    max
}

fn count_local_variables(node: Node, source: &str) -> usize {
    let mut vars = HashSet::new();
    collect_local_variables(node, source, &mut vars);
    vars.len()
}

fn collect_local_variables(node: Node, source: &str, vars: &mut HashSet<String>) {
    if (node.kind() == "assignment" || node.kind() == "augmented_assignment")
        && let Some(left) = node.child_by_field_name("left") { collect_assigned_names(left, source, vars); }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) { collect_local_variables(child, source, vars); }
}

fn collect_assigned_names(node: Node, source: &str, vars: &mut HashSet<String>) {
    match node.kind() {
        "identifier" => { if let Ok(name) = node.utf8_text(source.as_bytes()) { vars.insert(name.to_string()); } }
        "pattern_list" | "tuple_pattern" => { let mut c = node.walk(); for child in node.children(&mut c) { collect_assigned_names(child, source, vars); } }
        _ => {}
    }
}

fn compute_nested_function_depth(node: Node, current_depth: usize) -> usize {
    let is_fn = matches!(node.kind(), "function_definition" | "async_function_definition");
    let new_depth = if is_fn && current_depth > 0 { current_depth + 1 } else if is_fn { 1 } else { current_depth };
    let mut cursor = node.walk();
    let max = node.children(&mut cursor).fold(new_depth, |m, c| m.max(compute_nested_function_depth(c, new_depth)));
    if is_fn { max.saturating_sub(1) } else { max }
}

fn count_imports(node: Node) -> usize {
    let mut cursor = node.walk();
    node.children(&mut cursor).map(|c| match c.kind() {
        "import_statement" => 1,
        "import_from_statement" => count_import_names(c).max(1),
        _ => count_imports(c)
    }).sum()
}

fn count_import_names(node: Node) -> usize {
    let (mut count, mut seen_import) = (0, false);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import" => seen_import = true,
            "dotted_name" | "aliased_import" if seen_import => count += 1,
            _ => {}
        }
    }
    count
}

pub fn count_node_kind(node: Node, kind: &str) -> usize {
    let mut cursor = node.walk();
    usize::from(node.kind() == kind) + node.children(&mut cursor).map(|c| count_node_kind(c, kind)).sum::<usize>()
}

pub fn compute_lcom(body: Node, source: &str) -> f64 {
    const MIN_METHODS: usize = 2;
    let mut fields: Vec<HashSet<String>> = Vec::new();
    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        if child.kind() == "function_definition" { fields.push(extract_self_attributes(child, source)); }
    }
    if fields.len() < MIN_METHODS { return 0.0 }
    let total = fields.len() * (fields.len() - 1) / 2;
    let sharing = (0..fields.len()).flat_map(|i| ((i+1)..fields.len()).map(move |j| (i, j)))
        .filter(|(i, j)| !fields[*i].is_disjoint(&fields[*j])).count();
    (total - sharing) as f64 / total as f64
}

pub fn extract_self_attributes(node: Node, source: &str) -> HashSet<String> {
    let mut fields = HashSet::new();
    extract_self_attributes_recursive(node, source, &mut fields);
    fields
}

pub fn extract_self_attributes_recursive(node: Node, source: &str, fields: &mut HashSet<String>) {
    if node.kind() == "attribute"
        && let (Some(obj), Some(attr)) = (node.child_by_field_name("object"), node.child_by_field_name("attribute"))
        && obj.kind() == "identifier" && obj.utf8_text(source.as_bytes()).unwrap_or("") == "self"
        && let Ok(name) = attr.utf8_text(source.as_bytes()) { fields.insert(name.to_string()); }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) { extract_self_attributes_recursive(child, source, fields); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::{create_parser, parse_file};
    use std::io::Write;

    fn parse(code: &str) -> crate::parsing::ParsedFile {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "{code}").unwrap();
        parse_file(&mut create_parser().unwrap(), tmp.path()).unwrap()
    }

    #[test]
    fn test_function_metrics() {
        let p = parse("def f(a, b):\n    x = 1\n    return x");
        let m = compute_function_metrics(p.tree.root_node().child(0).unwrap(), &p.source);
        assert_eq!(m.arguments, 2);
        assert!(m.statements >= 2);
    }

    #[test]
    fn test_class_metrics() {
        let p = parse("class C:\n    def a(self): pass\n    def b(self): pass");
        assert_eq!(compute_class_metrics(p.tree.root_node().child(0).unwrap()).methods, 2);
    }

    #[test]
    fn test_file_metrics() {
        let p = parse("import os\nclass A: pass");
        let m = compute_file_metrics(&p);
        assert_eq!(m.classes, 1);
        assert!(m.imports >= 1);
    }

    #[test]
    fn test_keyword_only_args() {
        let p = parse("def f(a, b, c, *, d, e, f): pass");
        let m = compute_function_metrics(p.tree.root_node().child(0).unwrap(), &p.source);
        assert_eq!(m.arguments_positional, 3);
        assert_eq!(m.arguments_keyword_only, 3);
    }

    #[test]
    fn test_from_import_counts() {
        let p = parse("from typing import Any, List");
        assert_eq!(compute_file_metrics(&p).imports, 2);
    }

    #[test]
    fn test_lazy_imports() {
        let p = parse("import os\ndef f():\n    import numpy");
        assert_eq!(compute_file_metrics(&p).imports, 2);
    }

    #[test]
    fn test_try_block_statements() {
        let code = r"
def f():
    try:
        x = 1
        y = 2
        z = 3
    except Exception:
        pass
";
        let p = parse(code);
        let m = compute_function_metrics(p.tree.root_node().child(0).unwrap(), &p.source);
        assert_eq!(m.max_try_block_statements, 3);
    }

    #[test]
    fn test_nested_try_blocks() {
        let code = r"
def f():
    try:
        a = 1
    except:
        pass
    try:
        b = 1
        c = 2
        d = 3
        e = 4
    except:
        pass
";
        let p = parse(code);
        let m = compute_function_metrics(p.tree.root_node().child(0).unwrap(), &p.source);
        assert_eq!(m.max_try_block_statements, 4); // max of 1 and 4
    }
}
