
use crate::parsing::ParsedFile;
use std::collections::HashSet;
use tree_sitter::Node;

#[derive(Debug, Default)]
pub struct FunctionMetrics {
    pub statements: usize, pub arguments: usize, pub arguments_positional: usize, pub arguments_keyword_only: usize,
    pub max_indentation: usize, pub nested_function_depth: usize, pub returns: usize, pub branches: usize, pub local_variables: usize,
    pub max_try_block_statements: usize, pub boolean_parameters: usize, pub decorators: usize,
}

#[derive(Debug, Default)]
pub struct ClassMetrics { pub methods: usize }

#[derive(Debug, Default)]
pub struct FileMetrics { pub lines: usize, pub classes: usize, pub imports: usize }

#[must_use]
pub fn compute_function_metrics(node: Node, source: &str) -> FunctionMetrics {
    let mut m = FunctionMetrics::default();
    if let Some(params) = node.child_by_field_name("parameters") {
        let c = count_parameters(params, source);
        m.arguments = c.total; m.arguments_positional = c.positional; m.arguments_keyword_only = c.keyword_only;
        m.boolean_parameters = c.boolean_params;
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
    m.decorators = count_decorators(node);
    m
}

#[must_use]
pub fn compute_class_metrics(node: Node) -> ClassMetrics {
    let Some(body) = node.child_by_field_name("body") else { return ClassMetrics::default() };
    ClassMetrics { methods: count_node_kind(body, "function_definition") }
}

#[must_use]
pub fn compute_class_metrics_with_source(node: Node, source: &str) -> ClassMetrics {
    let _ = source;
    let Some(body) = node.child_by_field_name("body") else { return ClassMetrics::default() };
    ClassMetrics { methods: count_node_kind(body, "function_definition") }
}

#[must_use]
pub fn compute_file_metrics(parsed: &ParsedFile) -> FileMetrics {
    let root = parsed.tree.root_node();
    FileMetrics { lines: parsed.source.lines().count(), classes: count_node_kind(root, "class_definition"), imports: count_imports(root) }
}

struct ParameterCounts { positional: usize, keyword_only: usize, total: usize, boolean_params: usize }

fn count_parameters(params: Node, source: &str) -> ParameterCounts {
    let (mut positional, mut keyword_only, mut after_star, mut boolean_params) = (0, 0, false, 0);
    let mut cursor = params.walk();
    for child in params.children(&mut cursor) {
        match child.kind() {
            "identifier" | "typed_parameter" => if after_star { keyword_only += 1 } else { positional += 1 },
            "default_parameter" | "typed_default_parameter" => {
                if after_star { keyword_only += 1 } else { positional += 1 }
                if is_boolean_default(&child, source) { boolean_params += 1; }
            }
            "list_splat_pattern" | "dictionary_splat_pattern" | "*" | "keyword_separator" => after_star = true,
            _ => {}
        }
    }
    ParameterCounts { positional, keyword_only, total: positional + keyword_only, boolean_params }
}

fn is_boolean_default(param: &Node, source: &str) -> bool {
    param.child_by_field_name("value").is_some_and(|v| {
        let text = v.utf8_text(source.as_bytes()).unwrap_or("");
        matches!(text, "True" | "False")
    })
}

fn count_decorators(node: Node) -> usize {
    node.parent()
        .filter(|p| p.kind() == "decorated_definition")
        .map_or(0, |p| p.children(&mut p.walk()).filter(|c| c.kind() == "decorator").count())
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
    if node.kind() == "try_statement"
        && let Some(body) = node.child_by_field_name("body")
    {
        max = max.max(count_statements(body));
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
    let new_depth = if is_fn { current_depth + 1 } else { current_depth };
    let mut cursor = node.walk();
    let max = node.children(&mut cursor).fold(new_depth, |m, c| m.max(compute_nested_function_depth(c, new_depth)));
    if is_fn && current_depth == 0 { max.saturating_sub(1) } else { max }
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

    fn get_func_node(p: &crate::parsing::ParsedFile) -> Node<'_> { p.tree.root_node().child(0).unwrap() }

    #[test]
    fn test_params_and_decorators() {
        let p = parse("def f(a, b, c): pass");
        let params = get_func_node(&p).child_by_field_name("parameters").unwrap();
        assert_eq!(count_parameters(params, &p.source).positional, 3);
        let _ = ParameterCounts { positional: 2, keyword_only: 1, total: 3, boolean_params: 0 };
        let p2 = parse("def f(a=True): pass");
        let params2 = get_func_node(&p2).child_by_field_name("parameters").unwrap();
        let param = params2.children(&mut params2.walk()).find(|c| c.kind() == "default_parameter").unwrap();
        assert!(is_boolean_default(&param, &p2.source));
        assert_eq!(count_decorators(get_func_node(&parse("@dec\ndef f(): pass")).child(1).unwrap()), 1);
    }

    #[test]
    fn test_statements_and_branches() {
        let p = parse("def f():\n    x = 1\n    y = 2");
        let body = get_func_node(&p).child_by_field_name("body").unwrap();
        assert_eq!(count_statements(body), 2);
        assert!(is_statement("return_statement") && !is_statement("identifier"));
        assert_eq!(compute_max_indentation(body, 0), 0);
        assert_eq!(count_branches(get_func_node(&parse("def f():\n    if a: pass")).child_by_field_name("body").unwrap()), 1);
        assert_eq!(compute_max_try_block_statements(get_func_node(&parse("def f():\n    try:\n        x=1\n    except: pass")).child_by_field_name("body").unwrap()), 1);
    }

    #[test]
    fn test_variables_and_imports() {
        let p = parse("def f():\n    x = 1\n    y = 2");
        let body = get_func_node(&p).child_by_field_name("body").unwrap();
        assert_eq!(count_local_variables(body, &p.source), 2);
        let mut vars = HashSet::new();
        collect_local_variables(body, &p.source, &mut vars);
        let p2 = parse("x, y = 1, 2");
        let mut v2 = HashSet::new();
        collect_assigned_names(p2.tree.root_node().child(0).unwrap().child(0).unwrap().child_by_field_name("left").unwrap(), &p2.source, &mut v2);
        assert_eq!(compute_nested_function_depth(get_func_node(&parse("def f():\n    def g(): pass")), 0), 1);
        assert_eq!(count_imports(parse("import os").tree.root_node()), 1);
        assert_eq!(count_import_names(parse("from typing import Any, List").tree.root_node().child(0).unwrap()), 2);
    }
}
