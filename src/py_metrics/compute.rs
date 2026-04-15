use crate::parsing::ParsedFile;
use tree_sitter::Node;

use super::body_walk::analyze_body;
use super::file_stats::count_file_statements;
use super::file_walk::collect_file_counts;
use super::nesting::{compute_nested_function_depth, count_node_kind};
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
