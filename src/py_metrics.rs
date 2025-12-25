//! Python-specific metrics computation
//!
//! Compute function, class, and file metrics from Python AST nodes.

use crate::parsing::ParsedFile;
use std::collections::HashSet;
use tree_sitter::Node;

/// Metrics for a Python function
#[derive(Debug, Default)]
pub struct FunctionMetrics {
    pub statements: usize,
    pub arguments: usize,
    pub arguments_positional: usize,
    pub arguments_keyword_only: usize,
    pub max_indentation: usize,
    pub nested_function_depth: usize,
    pub returns: usize,
    pub branches: usize,
    pub local_variables: usize,
}

/// Metrics for a Python class
#[derive(Debug, Default)]
pub struct ClassMetrics {
    pub methods: usize,
    pub lcom: f64,
}

/// Metrics for a Python file
#[derive(Debug, Default)]
pub struct FileMetrics {
    pub lines: usize,
    pub classes: usize,
    pub imports: usize,
}

/// Compute metrics for a function node
#[must_use]
pub fn compute_function_metrics(node: Node, source: &str) -> FunctionMetrics {
    let mut metrics = FunctionMetrics::default();
    
    if let Some(params) = node.child_by_field_name("parameters") {
        let counts = count_parameters(params);
        metrics.arguments = counts.total;
        metrics.arguments_positional = counts.positional;
        metrics.arguments_keyword_only = counts.keyword_only;
    }
    
    if let Some(body) = node.child_by_field_name("body") {
        metrics.statements = count_statements(body);
        metrics.max_indentation = compute_max_indentation(body, 0);
        metrics.branches = count_branches(body);
        metrics.local_variables = count_local_variables(body, source);
        metrics.returns = count_node_kind(body, "return_statement");
    }
    
    metrics.nested_function_depth = compute_nested_function_depth(node, 0);
    metrics
}

/// Compute basic metrics for a class node (method count only)
#[must_use]
pub fn compute_class_metrics(node: Node) -> ClassMetrics {
    let body = match node.child_by_field_name("body") {
        Some(b) => b,
        None => return ClassMetrics::default(),
    };
    ClassMetrics { methods: count_node_kind(body, "function_definition"), lcom: 0.0 }
}

/// Compute metrics for a class node including LCOM
#[must_use]
pub fn compute_class_metrics_with_source(node: Node, source: &str) -> ClassMetrics {
    let body = match node.child_by_field_name("body") {
        Some(b) => b,
        None => return ClassMetrics::default(),
    };
    ClassMetrics {
        methods: count_node_kind(body, "function_definition"),
        lcom: compute_lcom(body, source),
    }
}

/// Compute metrics for a file
#[must_use]
pub fn compute_file_metrics(parsed: &ParsedFile) -> FileMetrics {
    let root = parsed.tree.root_node();
    FileMetrics {
        lines: parsed.source.lines().count(),
        classes: count_node_kind(root, "class_definition"),
        imports: count_imports(root),
    }
}

// --- Internal helpers ---

struct ParameterCounts {
    positional: usize,
    keyword_only: usize,
    total: usize,
}

fn count_parameters(params: Node) -> ParameterCounts {
    let mut positional = 0;
    let mut keyword_only = 0;
    let mut after_star = false;
    let mut cursor = params.walk();
    
    for child in params.children(&mut cursor) {
        match child.kind() {
            // *args and **kwargs don't count as regular parameters
            "list_splat_pattern" | "dictionary_splat_pattern" => {
                after_star = true; // *args also marks the boundary
            }
            "identifier" | "default_parameter" | "typed_parameter" | "typed_default_parameter" => {
                if after_star { keyword_only += 1; } else { positional += 1; }
            }
            // Bare * (keyword_separator) marks the boundary between positional and keyword-only
            "*" | "keyword_separator" => { after_star = true; }
            _ => {}
        }
    }
    ParameterCounts { positional, keyword_only, total: positional + keyword_only }
}

fn count_statements(node: Node) -> usize {
    let mut count = 0;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_statement(child.kind()) {
            count += 1;
            count += count_statements(child);
        } else {
            count += count_statements(child);
        }
    }
    count
}

fn is_statement(kind: &str) -> bool {
    matches!(kind,
        "expression_statement" | "return_statement" | "pass_statement" |
        "break_statement" | "continue_statement" | "raise_statement" |
        "assert_statement" | "global_statement" | "nonlocal_statement" |
        "import_statement" | "import_from_statement" | "future_import_statement" |
        "if_statement" | "for_statement" | "while_statement" | "try_statement" |
        "with_statement" | "match_statement" | "function_definition" |
        "class_definition" | "decorated_definition" | "async_function_definition" |
        "async_for_statement" | "async_with_statement" | "delete_statement" |
        "exec_statement" | "print_statement" | "type_alias_statement"
    )
}

fn compute_max_indentation(node: Node, current_depth: usize) -> usize {
    let depth_increase = matches!(node.kind(),
        "if_statement" | "for_statement" | "while_statement" |
        "try_statement" | "with_statement" | "match_statement" |
        "function_definition" | "class_definition" | "async_function_definition" |
        "async_for_statement" | "async_with_statement" | "elif_clause" |
        "else_clause" | "except_clause" | "finally_clause" | "case_clause"
    );
    
    let new_depth = if depth_increase { current_depth + 1 } else { current_depth };
    let mut max_depth = new_depth;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        max_depth = max_depth.max(compute_max_indentation(child, new_depth));
    }
    max_depth
}

fn count_branches(node: Node) -> usize {
    let mut count = 0;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(child.kind(), "if_statement" | "elif_clause") {
            count += 1;
        }
        count += count_branches(child);
    }
    count
}

fn count_local_variables(node: Node, source: &str) -> usize {
    let mut variables = HashSet::new();
    collect_local_variables(node, source, &mut variables);
    variables.len()
}

fn collect_local_variables(node: Node, source: &str, variables: &mut HashSet<String>) {
    if node.kind() == "assignment" || node.kind() == "augmented_assignment" {
        if let Some(left) = node.child_by_field_name("left") {
            collect_assigned_names(left, source, variables);
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_local_variables(child, source, variables);
    }
}

fn collect_assigned_names(node: Node, source: &str, variables: &mut HashSet<String>) {
    match node.kind() {
        "identifier" => {
            if let Ok(name) = node.utf8_text(source.as_bytes()) {
                variables.insert(name.to_string());
            }
        }
        "pattern_list" | "tuple_pattern" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_assigned_names(child, source, variables);
            }
        }
        _ => {}
    }
}

fn compute_nested_function_depth(node: Node, current_depth: usize) -> usize {
    let is_func = matches!(node.kind(), "function_definition" | "async_function_definition");
    let new_depth = if is_func && current_depth > 0 { current_depth + 1 } else if is_func { 1 } else { current_depth };
    let mut max_depth = new_depth;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        max_depth = max_depth.max(compute_nested_function_depth(child, new_depth));
    }
    if is_func { max_depth.saturating_sub(1) } else { max_depth }
}

fn count_imports(node: Node) -> usize {
    let mut count = 0;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_statement" => count += 1,
            "import_from_statement" => count += count_import_names(child).max(1),
            _ => count += count_imports(child),
        }
    }
    count
}

/// Count imported names in an import_from_statement.
/// For `from X import a, b, c`, counts a, b, c (not X).
fn count_import_names(node: Node) -> usize {
    let mut count = 0;
    let mut seen_import_keyword = false;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import" => seen_import_keyword = true,
            "dotted_name" | "aliased_import" if seen_import_keyword => count += 1,
            _ => {}
        }
    }
    count
}

/// Count nodes of a specific kind in the tree
pub fn count_node_kind(node: Node, kind: &str) -> usize {
    let mut count = if node.kind() == kind { 1 } else { 0 };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        count += count_node_kind(child, kind);
    }
    count
}

/// Compute LCOM (Lack of Cohesion of Methods) for a class body.
/// LCOM = pairs_not_sharing_fields / total_pairs. Returns 0.0 (cohesive) to 1.0 (no cohesion).
pub fn compute_lcom(body: Node, source: &str) -> f64 {
    let mut fields_per_method: Vec<HashSet<String>> = Vec::new();
    let mut cursor = body.walk();
    
    for child in body.children(&mut cursor) {
        if child.kind() == "function_definition" {
            fields_per_method.push(extract_self_attributes(child, source));
        }
    }
    
    const MIN_METHODS_FOR_LCOM: usize = 2;
    if fields_per_method.len() < MIN_METHODS_FOR_LCOM { return 0.0; }
    
    let total_pairs = fields_per_method.len() * (fields_per_method.len() - 1) / 2;
    let sharing_pairs = (0..fields_per_method.len())
        .flat_map(|i| ((i + 1)..fields_per_method.len()).map(move |j| (i, j)))
        .filter(|(i, j)| !fields_per_method[*i].is_disjoint(&fields_per_method[*j]))
        .count();
    
    (total_pairs - sharing_pairs) as f64 / total_pairs as f64
}

/// Extract self.field accesses from a method
pub fn extract_self_attributes(node: Node, source: &str) -> HashSet<String> {
    let mut fields = HashSet::new();
    extract_self_attributes_recursive(node, source, &mut fields);
    fields
}

pub fn extract_self_attributes_recursive(node: Node, source: &str, fields: &mut HashSet<String>) {
    if node.kind() == "attribute" {
        if let (Some(obj), Some(attr)) = (node.child_by_field_name("object"), node.child_by_field_name("attribute")) {
            if obj.kind() == "identifier" && obj.utf8_text(source.as_bytes()).unwrap_or("") == "self" {
                if let Ok(name) = attr.utf8_text(source.as_bytes()) {
                    fields.insert(name.to_string());
                }
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_self_attributes_recursive(child, source, fields);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::{create_parser, parse_file};
    use std::io::Write;

    fn parse_source(code: &str) -> ParsedFile {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "{}", code).unwrap();
        let mut parser = create_parser().unwrap();
        parse_file(&mut parser, tmp.path()).unwrap()
    }

    #[test]
    fn test_function_metrics() {
        let parsed = parse_source("def f(a, b):\n    x = 1\n    return x");
        let func = parsed.tree.root_node().child(0).unwrap();
        let m = compute_function_metrics(func, &parsed.source);
        assert_eq!(m.arguments, 2);
        assert!(m.statements >= 2);
    }

    #[test]
    fn test_class_metrics() {
        let parsed = parse_source("class C:\n    def a(self): pass\n    def b(self): pass");
        let cls = parsed.tree.root_node().child(0).unwrap();
        let m = compute_class_metrics(cls);
        assert_eq!(m.methods, 2);
    }

    #[test]
    fn test_file_metrics() {
        let parsed = parse_source("import os\nclass A: pass");
        let m = compute_file_metrics(&parsed);
        assert_eq!(m.classes, 1);
        assert!(m.imports >= 1);
    }

    #[test]
    fn test_lcom() {
        let parsed = parse_source("class C:\n    def a(self): self.x = 1\n    def b(self): self.y = 1");
        let cls = parsed.tree.root_node().child(0).unwrap();
        let body = cls.child_by_field_name("body").unwrap();
        let lcom = compute_lcom(body, &parsed.source);
        assert!(lcom >= 0.0 && lcom <= 1.0);
    }

    #[test]
    fn test_struct_defaults() {
        let fm = FunctionMetrics::default();
        assert_eq!(fm.statements, 0);
        let cm = ClassMetrics::default();
        assert_eq!(cm.methods, 0);
        let file = FileMetrics::default();
        assert_eq!(file.lines, 0);
    }

    #[test]
    fn test_compute_class_metrics_with_source() {
        let parsed = parse_source("class C:\n    def a(self): self.x = 1");
        let cls = parsed.tree.root_node().child(0).unwrap();
        let m = compute_class_metrics_with_source(cls, &parsed.source);
        assert_eq!(m.methods, 1);
    }

    #[test]
    fn test_helper_functions() {
        let _ = ParameterCounts { positional: 1, keyword_only: 2, total: 3 };
        let p = parse_source("def f(a, b): pass");
        let f = p.tree.root_node().child(0).unwrap();
        let params = f.child_by_field_name("parameters").unwrap();
        assert!(count_parameters(params).total >= 2);
        assert!(is_statement("return_statement") && !is_statement("identifier"));
        let p2 = parse_source("def f():\n    if True:\n        x = 1");
        let f2 = p2.tree.root_node().child(0).unwrap();
        if let Some(body) = f2.child_by_field_name("body") {
            let _ = (count_statements(body), compute_max_indentation(body, 0), count_branches(body), count_local_variables(body, &p2.source));
            let mut vars = HashSet::new(); collect_local_variables(body, &p2.source, &mut vars);
        }
        let p3 = parse_source("x = 1");
        if let Some(stmt) = p3.tree.root_node().child(0) { if let Some(left) = stmt.child_by_field_name("left") { let mut v = HashSet::new(); collect_assigned_names(left, &p3.source, &mut v); } }
        let p4 = parse_source("def f(): pass");
        let _ = compute_nested_function_depth(p4.tree.root_node().child(0).unwrap(), 0);
        let p5 = parse_source("import os");
        assert!(count_imports(p5.tree.root_node()) >= 1);
        let _ = count_import_names(p5.tree.root_node().child(0).unwrap());
    }

    #[test]
    fn test_count_node_kind() {
        let parsed = parse_source("def f(): pass\ndef g(): pass");
        let root = parsed.tree.root_node();
        assert_eq!(count_node_kind(root, "function_definition"), 2);
    }

    #[test]
    fn test_keyword_only_args_after_bare_star() {
        // Regression test: bare * should mark boundary between positional and keyword-only args
        let parsed = parse_source("def f(a, b, c, *, d, e, f): pass");
        let func = parsed.tree.root_node().child(0).unwrap();
        let m = compute_function_metrics(func, &parsed.source);
        assert_eq!(m.arguments_positional, 3, "a, b, c are positional");
        assert_eq!(m.arguments_keyword_only, 3, "d, e, f are keyword-only");
        assert_eq!(m.arguments, 6, "total arguments");
    }

    #[test]
    fn test_keyword_only_args_with_typed_params() {
        // Regression test: typed parameters after * should be keyword-only
        let parsed = parse_source(
            "def subsample_loglik(model: Any, x: Any, y: Any, *, paramss: list, P: int = 10, rng: Any) -> list: pass"
        );
        let func = parsed.tree.root_node().child(0).unwrap();
        let m = compute_function_metrics(func, &parsed.source);
        assert_eq!(m.arguments_positional, 3, "model, x, y are positional");
        assert_eq!(m.arguments_keyword_only, 3, "paramss, P, rng are keyword-only");
    }

    #[test]
    fn test_from_import_counts_names_not_module() {
        // Regression test: `from X import a, b` should count 2 (a, b), not 3 (X, a, b)
        let code = "from typing import Any, List";
        let parsed = parse_source(code);
        let m = compute_file_metrics(&parsed);
        assert_eq!(m.imports, 2, "from X import a, b should count imported names (2), not module name");
    }

    #[test]
    fn test_imports_counts_all_including_lazy() {
        // Imports inside functions (lazy imports) are still dependencies and should be counted
        let code = r#"
import os
from typing import Any, List

def my_function():
    import numpy as np
    from collections import deque
    pass
"#;
        let parsed = parse_source(code);
        let m = compute_file_metrics(&parsed);
        // os (1) + Any, List (2) + numpy (1) + deque (1) = 5
        assert_eq!(m.imports, 5, "should count all imports including lazy imports inside functions");
    }

    #[test]
    fn test_extract_self_attributes() {
        let parsed = parse_source("class C:\n    def m(self): self.x = 1; self.y = 2");
        let cls = parsed.tree.root_node().child(0).unwrap();
        let body = cls.child_by_field_name("body").unwrap();
        let method = body.child(0).unwrap();
        let fields = extract_self_attributes(method, &parsed.source);
        assert!(fields.len() >= 1);
    }

    #[test]
    fn test_extract_self_attributes_recursive() {
        let parsed = parse_source("class C:\n    def m(self): self.a = self.b");
        let cls = parsed.tree.root_node().child(0).unwrap();
        let body = cls.child_by_field_name("body").unwrap();
        let mut fields = HashSet::new();
        extract_self_attributes_recursive(body, &parsed.source, &mut fields);
        assert!(!fields.is_empty());
    }
}

