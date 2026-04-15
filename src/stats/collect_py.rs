use crate::py_metrics::{compute_class_metrics, compute_function_metrics};
use tree_sitter::Node;

use super::metric_stats::MetricStats;

// inside_class tracks context for method counting; passed through recursion to nested scopes
#[allow(clippy::only_used_in_recursion)]
pub(crate) fn collect_from_node(node: Node, source: &str, stats: &mut MetricStats, inside_class: bool) {
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            let m = compute_function_metrics(node, source);
            if !m.has_error {
                push_py_fn_metrics(stats, &m);
            }
            let mut c = node.walk();
            for child in node.children(&mut c) {
                collect_from_node(child, source, stats, false);
            }
        }
        "class_definition" => {
            let m = compute_class_metrics(node);
            stats.methods_per_class.push(m.methods);
            let mut c = node.walk();
            for child in node.children(&mut c) {
                collect_from_node(child, source, stats, true);
            }
        }
        _ => {
            let mut c = node.walk();
            for child in node.children(&mut c) {
                collect_from_node(child, source, stats, inside_class);
            }
        }
    }
}

pub(crate) fn push_py_fn_metrics(stats: &mut MetricStats, m: &crate::py_metrics::FunctionMetrics) {
    stats.statements_per_function.push(m.statements);
    stats.arguments_per_function.push(m.arguments);
    stats.arguments_positional.push(m.arguments_positional);
    stats.arguments_keyword_only.push(m.arguments_keyword_only);
    stats.max_indentation.push(m.max_indentation);
    stats.nested_function_depth.push(m.nested_function_depth);
    stats.returns_per_function.push(m.returns);
    stats.return_values_per_function.push(m.max_return_values);
    stats.branches_per_function.push(m.branches);
    stats.local_variables_per_function.push(m.local_variables);
    stats
        .statements_per_try_block
        .push(m.max_try_block_statements);
    stats.boolean_parameters.push(m.boolean_parameters);
    stats.annotations_per_function.push(m.decorators);
    stats.calls_per_function.push(m.calls);
}
