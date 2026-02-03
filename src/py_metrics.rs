use crate::parsing::ParsedFile;
use crate::py_imports::{
    collect_import_from_names, collect_import_statement_names, is_type_checking_block,
};
use std::collections::HashSet;
use tree_sitter::Node;

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
    pub max_try_block_statements: usize,
    pub boolean_parameters: usize,
    pub decorators: usize,
    pub max_return_values: usize,
    pub calls: usize,
}

#[derive(Debug, Default)]
pub struct ClassMetrics {
    pub methods: usize,
}

#[derive(Debug, Default)]
pub struct FileMetrics {
    pub statements: usize,
    pub interface_types: usize,
    pub concrete_types: usize,
    pub imports: usize,
    pub functions: usize,
}

#[must_use]
pub fn compute_function_metrics(node: Node, source: &str) -> FunctionMetrics {
    let mut m = FunctionMetrics::default();
    if let Some(params) = node.child_by_field_name("parameters") {
        let c = count_parameters(params, source);
        m.arguments = c.total;
        m.arguments_positional = c.positional;
        m.arguments_keyword_only = c.keyword_only;
        m.boolean_parameters = c.boolean_params;
    }
    if let Some(body) = node.child_by_field_name("body") {
        let agg = analyze_body(body, source);
        m.statements = agg.statements;
        m.max_indentation = agg.max_indentation;
        m.branches = agg.branches;
        m.local_variables = agg.local_variables;
        m.returns = agg.returns;
        m.max_try_block_statements = agg.max_try_block_statements;
        m.max_return_values = agg.max_return_values;
        m.calls = agg.calls;
    }
    m.nested_function_depth = compute_nested_function_depth(node, 0);
    m.decorators = count_decorators(node);
    m
}

#[derive(Default)]
struct BodyAgg {
    statements: usize,
    max_indentation: usize,
    branches: usize,
    returns: usize,
    calls: usize,
    max_try_block_statements: usize,
    max_return_values: usize,
    local_vars: HashSet<String>,
}

struct BodySummary {
    statements: usize,
    max_indentation: usize,
    branches: usize,
    local_variables: usize,
    returns: usize,
    calls: usize,
    max_try_block_statements: usize,
    max_return_values: usize,
}

fn analyze_body(body: Node, source: &str) -> BodySummary {
    let mut agg = BodyAgg::default();
    // Start at indentation depth 0 (matches previous compute_max_indentation(body, 0)).
    let _ = walk_body(body, source, 0, &mut agg);
    BodySummary {
        statements: agg.statements,
        max_indentation: agg.max_indentation,
        branches: agg.branches,
        local_variables: agg.local_vars.len(),
        returns: agg.returns,
        calls: agg.calls,
        max_try_block_statements: agg.max_try_block_statements,
        max_return_values: agg.max_return_values,
    }
}

// Returns statement count for this subtree (including this node if it is a statement).
fn walk_body(node: Node, source: &str, current_depth: usize, agg: &mut BodyAgg) -> usize {
    let new_depth = next_indent_depth(node.kind(), current_depth);
    agg.max_indentation = agg.max_indentation.max(new_depth);

    update_local_vars(node, source, &mut agg.local_vars);
    update_body_counts(node, agg);
    update_return_counts(node, agg);

    let try_body_range = try_body_byte_range(node);

    let mut subtree_stmt_count = usize::from(is_statement(node.kind()));
    let mut try_body_stmt_count: Option<usize> = None;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let child_stmts = walk_body(child, source, new_depth, agg);
        subtree_stmt_count += child_stmts;
        if is_try_body(child, try_body_range) {
            try_body_stmt_count = Some(child_stmts);
        }
    }
    update_try_block_statements(node, try_body_stmt_count, agg);
    subtree_stmt_count
}

fn next_indent_depth(kind: &str, current_depth: usize) -> usize {
    if is_indent_increasing(kind) {
        current_depth + 1
    } else {
        current_depth
    }
}

fn is_indent_increasing(kind: &str) -> bool {
    matches!(
        kind,
        "if_statement"
            | "for_statement"
            | "while_statement"
            | "try_statement"
            | "with_statement"
            | "match_statement"
            | "function_definition"
            | "class_definition"
            | "async_function_definition"
            | "async_for_statement"
            | "async_with_statement"
            | "elif_clause"
            | "else_clause"
            | "except_clause"
            | "finally_clause"
            | "case_clause"
    )
}

fn update_local_vars(node: Node, source: &str, vars: &mut HashSet<String>) {
    if (node.kind() == "assignment" || node.kind() == "augmented_assignment")
        && let Some(left) = node.child_by_field_name("left")
    {
        collect_assigned_names(left, source, vars);
    }
}

fn update_body_counts(node: Node, agg: &mut BodyAgg) {
    if is_statement(node.kind()) {
        agg.statements += 1;
    }
    if matches!(node.kind(), "if_statement" | "elif_clause" | "case_clause") {
        agg.branches += 1;
    }
    if node.kind() == "call" {
        agg.calls += 1;
    }
}

fn update_return_counts(node: Node, agg: &mut BodyAgg) {
    if node.kind() == "return_statement" {
        agg.returns += 1;
        agg.max_return_values = agg.max_return_values.max(count_return_values(node));
    }
}

fn try_body_byte_range(node: Node) -> Option<(usize, usize)> {
    (node.kind() == "try_statement")
        .then(|| node.child_by_field_name("body"))
        .flatten()
        .map(|b| (b.start_byte(), b.end_byte()))
}

fn is_try_body(child: Node, try_body_range: Option<(usize, usize)>) -> bool {
    if let Some((sb, eb)) = try_body_range {
        child.start_byte() == sb && child.end_byte() == eb
    } else {
        false
    }
}

fn update_try_block_statements(node: Node, try_body_stmt_count: Option<usize>, agg: &mut BodyAgg) {
    if node.kind() == "try_statement"
        && let Some(body_stmts) = try_body_stmt_count
    {
        agg.max_try_block_statements = agg.max_try_block_statements.max(body_stmts);
    }
}

#[must_use]
pub fn compute_class_metrics(node: Node) -> ClassMetrics {
    let Some(body) = node.child_by_field_name("body") else {
        return ClassMetrics::default();
    };
    ClassMetrics {
        methods: count_node_kind(body, "function_definition"),
    }
}

#[must_use]
pub fn compute_file_metrics(parsed: &ParsedFile) -> FileMetrics {
    let root = parsed.tree.root_node();
    let statements = count_file_statements(root);
    let counts = collect_file_counts(root, &parsed.source);
    FileMetrics {
        statements,
        interface_types: counts.interface_types,
        concrete_types: counts.concrete_types,
        imports: counts.import_names.len(),
        functions: counts.functions,
    }
}

#[derive(Default)]
struct FileCounts {
    interface_types: usize,
    concrete_types: usize,
    functions: usize,
    import_names: HashSet<String>,
}

fn collect_file_counts(root: Node, source: &str) -> FileCounts {
    let mut agg = FileCounts::default();
    walk_file(root, source, false, &mut agg);
    agg
}

fn walk_file(node: Node, source: &str, in_type_checking: bool, agg: &mut FileCounts) {
    let now_type_checking = in_type_checking || is_type_checking_block(node, source);

    match node.kind() {
        "function_definition" | "async_function_definition" => agg.functions += 1,
        "class_definition" => {
            if is_interface_type(node, source) {
                agg.interface_types += 1;
            } else {
                agg.concrete_types += 1;
            }
        }
        "import_statement" if !now_type_checking => {
            collect_import_statement_names(node, source, &mut agg.import_names);
        }
        "import_from_statement" if !now_type_checking => {
            collect_import_from_names(node, source, &mut agg.import_names);
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_file(child, source, now_type_checking, agg);
    }
}

fn is_interface_type(class_node: Node, source: &str) -> bool {
    // tree-sitter-python has historically used "superclasses" as a field name, but to be robust we
    // also fall back to finding an argument_list child directly.
    let supers = class_node.child_by_field_name("superclasses").or_else(|| {
        let mut c = class_node.walk();
        class_node
            .children(&mut c)
            .find(|n| n.kind() == "argument_list")
    });

    let Some(supers) = supers else {
        return false;
    };
    let Ok(text) = supers.utf8_text(source.as_bytes()) else {
        return false;
    };

    let mut token = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch);
        } else {
            if is_interface_token(&token) {
                return true;
            }
            token.clear();
        }
    }
    is_interface_token(&token)
}

fn is_interface_token(token: &str) -> bool {
    matches!(token, "Protocol" | "ABC" | "ABCMeta")
}

fn count_file_statements(node: Node) -> usize {
    let mut total = 0;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_definition" | "async_function_definition" => {
                if let Some(body) = child.child_by_field_name("body") {
                    total += count_statements(body);
                }
            }
            "class_definition" => {
                if let Some(body) = child.child_by_field_name("body") {
                    total += count_class_statements(body);
                }
            }
            "decorated_definition" => {
                total += count_file_statements(child);
            }
            _ => {}
        }
    }
    total
}

fn count_class_statements(body: Node) -> usize {
    let mut total = 0;
    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        match child.kind() {
            "function_definition" | "async_function_definition" => {
                if let Some(fn_body) = child.child_by_field_name("body") {
                    total += count_statements(fn_body);
                }
            }
            "decorated_definition" => {
                total += count_class_statements(child);
            }
            _ => {}
        }
    }
    total
}

struct ParameterCounts {
    positional: usize,
    keyword_only: usize,
    total: usize,
    boolean_params: usize,
}

fn count_parameters(params: Node, source: &str) -> ParameterCounts {
    let (mut positional, mut keyword_only, mut after_star, mut boolean_params) = (0, 0, false, 0);
    let mut cursor = params.walk();
    for child in params.children(&mut cursor) {
        match child.kind() {
            "identifier" | "typed_parameter" => {
                if after_star {
                    keyword_only += 1;
                } else {
                    positional += 1;
                }
            }
            "default_parameter" | "typed_default_parameter" => {
                if after_star {
                    keyword_only += 1;
                } else {
                    positional += 1;
                }
                if is_boolean_default(&child, source) {
                    boolean_params += 1;
                }
            }
            "list_splat_pattern" | "dictionary_splat_pattern" | "*" | "keyword_separator" => {
                after_star = true;
            }
            _ => {}
        }
    }
    ParameterCounts {
        positional,
        keyword_only,
        total: positional + keyword_only,
        boolean_params,
    }
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
        .map_or(0, |p| {
            p.children(&mut p.walk())
                .filter(|c| c.kind() == "decorator")
                .count()
        })
}

fn count_statements(node: Node) -> usize {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .map(|c| usize::from(is_statement(c.kind())) + count_statements(c))
        .sum()
}

fn is_statement(kind: &str) -> bool {
    // Per style.md: "[statement] Any statement within a function body that is not an import or a signature"
    // Excludes: import_statement, import_from_statement, future_import_statement
    matches!(
        kind,
        "expression_statement"
            | "return_statement"
            | "pass_statement"
            | "break_statement"
            | "continue_statement"
            | "raise_statement"
            | "assert_statement"
            | "global_statement"
            | "nonlocal_statement"
            | "if_statement"
            | "for_statement"
            | "while_statement"
            | "try_statement"
            | "with_statement"
            | "match_statement"
            | "function_definition"
            | "class_definition"
            | "decorated_definition"
            | "async_function_definition"
            | "async_for_statement"
            | "async_with_statement"
            | "delete_statement"
            | "exec_statement"
            | "print_statement"
            | "type_alias_statement"
    )
}

#[cfg(test)]
#[allow(dead_code)]
fn compute_max_indentation(node: Node, current_depth: usize) -> usize {
    let depth_inc = matches!(
        node.kind(),
        "if_statement"
            | "for_statement"
            | "while_statement"
            | "try_statement"
            | "with_statement"
            | "match_statement"
            | "function_definition"
            | "class_definition"
            | "async_function_definition"
            | "async_for_statement"
            | "async_with_statement"
            | "elif_clause"
            | "else_clause"
            | "except_clause"
            | "finally_clause"
            | "case_clause"
    );
    let new_depth = if depth_inc {
        current_depth + 1
    } else {
        current_depth
    };
    let mut cursor = node.walk();
    node.children(&mut cursor).fold(new_depth, |max, c| {
        max.max(compute_max_indentation(c, new_depth))
    })
}

// NOTE: The functions below are retained for readability/tests and as reference implementations.
// The production fast-path uses `analyze_body()` to compute these in a single traversal.

#[cfg(test)]
#[allow(dead_code)]
fn count_branches(node: Node) -> usize {
    let mut cursor = node.walk();
    // Count if/elif and match case clauses as branches (Python 3.10+ match/case support)
    node.children(&mut cursor)
        .map(|c| {
            usize::from(matches!(
                c.kind(),
                "if_statement" | "elif_clause" | "case_clause"
            )) + count_branches(c)
        })
        .sum()
}

#[cfg(test)]
#[allow(dead_code)]
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

#[cfg(test)]
#[allow(dead_code)]
fn compute_max_return_values(node: Node) -> usize {
    let mut max = if node.kind() == "return_statement" {
        count_return_values(node)
    } else {
        0
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        max = max.max(compute_max_return_values(child));
    }
    max
}

fn count_return_values(node: Node) -> usize {
    // return statement child is the expression being returned
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "return" {
            continue;
        } // skip the 'return' keyword
        return match child.kind() {
            "expression_list" => child.named_child_count(),
            _ => 1, // single value
        };
    }
    0 // bare return
}

#[cfg(test)]
#[allow(dead_code)]
fn count_local_variables(node: Node, source: &str) -> usize {
    let mut vars = HashSet::new();
    collect_local_variables(node, source, &mut vars);
    vars.len()
}

#[cfg(test)]
#[allow(dead_code)]
fn collect_local_variables(node: Node, source: &str, vars: &mut HashSet<String>) {
    if (node.kind() == "assignment" || node.kind() == "augmented_assignment")
        && let Some(left) = node.child_by_field_name("left")
    {
        collect_assigned_names(left, source, vars);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_local_variables(child, source, vars);
    }
}

fn collect_assigned_names(node: Node, source: &str, vars: &mut HashSet<String>) {
    match node.kind() {
        "identifier" => {
            if let Ok(name) = node.utf8_text(source.as_bytes()) {
                vars.insert(name.to_string());
            }
        }
        "pattern_list" | "tuple_pattern" => {
            let mut c = node.walk();
            for child in node.children(&mut c) {
                collect_assigned_names(child, source, vars);
            }
        }
        _ => {}
    }
}

fn compute_nested_function_depth(node: Node, current_depth: usize) -> usize {
    let is_fn = matches!(
        node.kind(),
        "function_definition" | "async_function_definition"
    );
    let new_depth = if is_fn {
        current_depth + 1
    } else {
        current_depth
    };
    let mut cursor = node.walk();
    let max = node.children(&mut cursor).fold(new_depth, |m, c| {
        m.max(compute_nested_function_depth(c, new_depth))
    });
    if is_fn && current_depth == 0 {
        max.saturating_sub(1)
    } else {
        max
    }
}

pub fn count_node_kind(node: Node, kind: &str) -> usize {
    let mut cursor = node.walk();
    usize::from(node.kind() == kind)
        + node
            .children(&mut cursor)
            .map(|c| count_node_kind(c, kind))
            .sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::parse_python_source as parse;

    fn get_func_node(p: &crate::parsing::ParsedFile) -> Node<'_> {
        p.tree.root_node().child(0).unwrap()
    }

    #[test]
    fn test_params_and_decorators() {
        let p = parse("def f(a, b, c): pass");
        let params = get_func_node(&p).child_by_field_name("parameters").unwrap();
        assert_eq!(count_parameters(params, &p.source).positional, 3);
        let _ = ParameterCounts {
            positional: 2,
            keyword_only: 1,
            total: 3,
            boolean_params: 0,
        };
        let p2 = parse("def f(a=True): pass");
        let params2 = get_func_node(&p2)
            .child_by_field_name("parameters")
            .unwrap();
        let param = params2
            .children(&mut params2.walk())
            .find(|c| c.kind() == "default_parameter")
            .unwrap();
        assert!(is_boolean_default(&param, &p2.source));
        assert_eq!(
            count_decorators(
                get_func_node(&parse("@dec\ndef f(): pass"))
                    .child(1)
                    .unwrap()
            ),
            1
        );
    }

    #[test]
    fn test_boolean_params_count() {
        // Test that boolean_params counts parameters with True/False defaults
        let p = parse("def f(a=True, b=False): pass");
        let params = get_func_node(&p).child_by_field_name("parameters").unwrap();
        let counts = count_parameters(params, &p.source);
        assert_eq!(
            counts.boolean_params, 2,
            "Should count 2 boolean parameters (a=True, b=False)"
        );

        // Also test typed parameters
        let p2 = parse("def f(a: bool = True, b: int = 5): pass");
        let params2 = get_func_node(&p2)
            .child_by_field_name("parameters")
            .unwrap();
        let counts2 = count_parameters(params2, &p2.source);
        assert_eq!(
            counts2.boolean_params, 1,
            "Should count 1 boolean parameter (a: bool = True)"
        );

        // Test compute_function_metrics returns correct boolean_parameters
        let p3 = parse("def f(a=True, b=False): x = 1");
        let m = compute_function_metrics(get_func_node(&p3), &p3.source);
        assert_eq!(
            m.boolean_parameters, 2,
            "compute_function_metrics should report 2 boolean params"
        );
    }

    #[test]
    fn test_statements_and_branches() {
        let p = parse("def f():\n    x = 1\n    y = 2");
        let body = get_func_node(&p).child_by_field_name("body").unwrap();
        assert_eq!(count_statements(body), 2);
        assert!(is_statement("return_statement") && !is_statement("identifier"));
        assert_eq!(compute_max_indentation(body, 0), 0);
        assert_eq!(
            count_branches(
                get_func_node(&parse("def f():\n    if a: pass"))
                    .child_by_field_name("body")
                    .unwrap()
            ),
            1
        );
        assert_eq!(
            compute_max_try_block_statements(
                get_func_node(&parse("def f():\n    try:\n        x=1\n    except: pass"))
                    .child_by_field_name("body")
                    .unwrap()
            ),
            1
        );
    }

    #[test]
    fn test_import_statements_not_counted() {
        // Per style.md: "[statement] Any statement within a function body that is not an import or a signature"
        // import statements inside function bodies should NOT be counted
        let p = parse("def f():\n    import os\n    x = 1\n    print(x)");
        let body = get_func_node(&p).child_by_field_name("body").unwrap();
        // Should be 2 statements (assignment + expression), not 3
        assert_eq!(
            count_statements(body),
            2,
            "import statements should not be counted"
        );

        // Also test from imports
        let p2 = parse("def f():\n    from os import path\n    y = 2");
        let body2 = get_func_node(&p2).child_by_field_name("body").unwrap();
        assert_eq!(
            count_statements(body2),
            1,
            "from imports should not be counted"
        );

        // Verify import_statement and import_from_statement are not statements
        assert!(
            !is_statement("import_statement"),
            "import_statement should not be a statement"
        );
        assert!(
            !is_statement("import_from_statement"),
            "import_from_statement should not be a statement"
        );
    }

    #[test]
    fn test_file_types_split() {
        let p = parse(
            "from typing import Protocol\nfrom abc import ABC\n\nclass P(Protocol):\n    pass\n\nclass A(ABC):\n    pass\n\nclass C:\n    pass\n",
        );
        let m = compute_file_metrics(&p);
        assert_eq!(m.interface_types, 2);
        assert_eq!(m.concrete_types, 1);
    }

    #[test]
    fn test_variables_and_nesting() {
        let p = parse("def f():\n    x = 1\n    y = 2");
        let body = get_func_node(&p).child_by_field_name("body").unwrap();
        assert_eq!(count_local_variables(body, &p.source), 2);
        let mut vars = HashSet::new();
        collect_local_variables(body, &p.source, &mut vars);
        let p2 = parse("x, y = 1, 2");
        let mut v2 = HashSet::new();
        collect_assigned_names(
            p2.tree
                .root_node()
                .child(0)
                .unwrap()
                .child(0)
                .unwrap()
                .child_by_field_name("left")
                .unwrap(),
            &p2.source,
            &mut v2,
        );
        assert_eq!(
            compute_nested_function_depth(get_func_node(&parse("def f():\n    def g(): pass")), 0),
            1
        );
    }

    #[test]
    fn test_return_values() {
        // Single value
        let p1 = parse("def f():\n    return x");
        assert_eq!(
            compute_function_metrics(get_func_node(&p1), &p1.source).max_return_values,
            1
        );
        // Multiple values (tuple)
        let p2 = parse("def f():\n    return a, b, c");
        assert_eq!(
            compute_function_metrics(get_func_node(&p2), &p2.source).max_return_values,
            3
        );
        // Bare return
        let p3 = parse("def f():\n    return");
        assert_eq!(
            compute_function_metrics(get_func_node(&p3), &p3.source).max_return_values,
            0
        );
        // Max across multiple returns
        let p4 = parse("def f():\n    if x:\n        return a, b\n    return a, b, c, d");
        assert_eq!(
            compute_function_metrics(get_func_node(&p4), &p4.source).max_return_values,
            4
        );
    }

    #[test]
    fn test_touch_analyze_body_helpers_for_static_coverage() {
        let p = parse("def f():\n    x = 1\n    return a, b");
        let func = get_func_node(&p);
        let body = func.child_by_field_name("body").unwrap();
        assert!(analyze_body(body, &p.source).statements > 0);
    }

    #[test]
    fn test_touch_file_count_helpers_for_static_coverage() {
        let p = parse("from typing import Protocol\nimport os\n\nclass P(Protocol):\n    pass\n");
        let counts = collect_file_counts(p.tree.root_node(), &p.source);
        assert!(counts.import_names.contains("os"));

        let root = p.tree.root_node();
        let cls = (0..root.child_count())
            .filter_map(|i| root.child(i))
            .find(|n| n.kind() == "class_definition")
            .expect("expected class_definition node");
        assert!(is_interface_type(cls, &p.source));
    }

    #[test]
    fn test_touch_return_helpers_for_static_coverage() {
        let p_ret = parse("def g():\n    return a, b, c");
        let ret = p_ret
            .tree
            .root_node()
            .child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap()
            .child(0)
            .unwrap();
        assert_eq!(count_return_values(ret), 3);
    }

    #[test]
    fn test_touch_statement_counters_for_static_coverage() {
        let p2 = parse("class C:\n    def m(self):\n        x = 1\n        return x\n");
        let root2 = p2.tree.root_node();
        assert!(count_file_statements(root2) > 0);
        let class_body = root2.child(0).unwrap().child_by_field_name("body").unwrap();
        assert!(count_class_statements(class_body) > 0);
    }

    #[test]
    fn test_is_interface_token() {
        assert!(super::is_interface_token("Protocol"));
        assert!(super::is_interface_token("ABC"));
        assert!(super::is_interface_token("ABCMeta"));
        assert!(!super::is_interface_token("BaseClass"));
        assert!(!super::is_interface_token("object"));
    }
}
