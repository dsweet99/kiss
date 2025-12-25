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
            "list_splat_pattern" | "dictionary_splat_pattern" => {}
            "identifier" | "default_parameter" | "typed_parameter" | "typed_default_parameter" => {
                if after_star { keyword_only += 1; } else { positional += 1; }
            }
            "*" => { after_star = true; }
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

fn count_import_names(node: Node) -> usize {
    let mut count = 0;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(child.kind(), "dotted_name" | "aliased_import") {
            count += 1;
        } else {
            count += count_import_names(child);
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

/// Compute LCOM (Lack of Cohesion of Methods) for a class body
pub fn compute_lcom(body: Node, source: &str) -> f64 {
    let mut method_fields: Vec<HashSet<String>> = Vec::new();
    let mut cursor = body.walk();
    
    for child in body.children(&mut cursor) {
        if child.kind() == "function_definition" {
            let fields = extract_self_attributes(child, source);
            if !fields.is_empty() {
                method_fields.push(fields);
            }
        }
    }
    
    let n = method_fields.len();
    if n < 2 { return 0.0; }
    
    let total_pairs = n * (n - 1) / 2;
    let mut sharing_pairs = 0;
    
    for i in 0..n {
        for j in (i + 1)..n {
            if !method_fields[i].is_disjoint(&method_fields[j]) {
                sharing_pairs += 1;
            }
        }
    }
    
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
}

