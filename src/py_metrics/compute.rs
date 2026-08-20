use crate::parsing::ParsedFile;
use tree_sitter::Node;

use super::body_walk::analyze_body;
use super::file_stats::count_file_statements;
use super::file_walk::collect_file_counts;
use super::nesting::compute_nested_function_depth;
use super::parameters::{count_decorators, count_parameters};
use super::types::{ClassMetrics, FileMetrics, FunctionMetrics};

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
    m.has_error = node.has_error();
    m
}

#[must_use]
pub fn compute_class_metrics(node: Node) -> ClassMetrics {
    let Some(body) = node.child_by_field_name("body") else {
        return ClassMetrics::default();
    };
    ClassMetrics {
        methods: count_direct_class_methods(body),
    }
}

fn count_direct_class_methods(body: Node) -> usize {
    let mut cursor = body.walk();
    body.named_children(&mut cursor)
        .filter(|child| is_direct_method(*child))
        .count()
}

fn is_direct_method(node: Node) -> bool {
    match node.kind() {
        "function_definition" | "async_function_definition" => true,
        "decorated_definition" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor).any(|child| {
                matches!(
                    child.kind(),
                    "function_definition" | "async_function_definition"
                )
            })
        }
        _ => false,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::parse_python_source;

    #[test]
    fn function_metrics_include_keyword_only_boolean_return_and_call_counts() {
        let parsed =
            parse_python_source("def f(a, *, flag=False):\n    helper()\n    return a, flag\n");
        let func = parsed.tree.root_node().child(0).expect("function");
        let metrics = compute_function_metrics(func, &parsed.source);

        assert_eq!(metrics.arguments_keyword_only, 1);
        assert_eq!(metrics.boolean_parameters, 1);
        assert_eq!(metrics.max_return_values, 2);
        assert_eq!(metrics.calls, 1);
    }

    #[test]
    fn class_and_file_metrics_count_methods_and_top_level_functions() {
        let parsed = parse_python_source(
            "def top():\n    return 1\n\nclass C:\n    def method(self):\n        return 2\n",
        );
        let root = parsed.tree.root_node();
        let class_node = root
            .children(&mut root.walk())
            .find(|node| node.kind() == "class_definition")
            .expect("class");

        assert_eq!(compute_class_metrics(class_node).methods, 1);
        assert_eq!(compute_file_metrics(&parsed).functions, 2);
    }

    #[test]
    fn class_metrics_ignore_nested_helpers() {
        let parsed = parse_python_source(
            "class C:\n    def method(self):\n        def helper():\n            return 1\n        return helper()\n",
        );
        let root = parsed.tree.root_node();
        let class_node = root
            .children(&mut root.walk())
            .find(|node| node.kind() == "class_definition")
            .expect("class");
        assert_eq!(compute_class_metrics(class_node).methods, 1);
    }

    #[test]
    fn match_case_tree_has_case_clause() {
        let parsed =
            parse_python_source("def f(x):\n    match x:\n        case 1:\n            return 1\n");
        fn kinds(node: tree_sitter::Node) -> Vec<(String, usize)> {
            let mut out = vec![(node.kind().to_string(), node.start_position().row + 1)];
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                out.extend(kinds(child));
            }
            out
        }
        let found = kinds(parsed.tree.root_node());
        assert!(
            found.iter().any(|(k, _)| k == "case_clause"),
            "kinds={found:?}"
        );
    }
}
