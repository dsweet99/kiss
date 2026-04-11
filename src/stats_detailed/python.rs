use crate::graph::DependencyGraph;
use crate::parsing::ParsedFile;
use crate::py_metrics::{
    compute_class_metrics, compute_file_metrics, compute_function_metrics, FunctionMetrics,
};
use tree_sitter::Node;

use super::types::UnitMetrics;
use super::file_unit_metrics;

pub fn collect_detailed_py(
    parsed_files: &[&ParsedFile],
    graph: Option<&DependencyGraph>,
) -> Vec<UnitMetrics> {
    let mut units = Vec::new();
    for parsed in parsed_files {
        let fm = compute_file_metrics(parsed);
        let lines = parsed.source.lines().count();
        units.push(file_unit_metrics(&parsed.path, lines, fm.imports, graph));
        collect_detailed_from_node(
            parsed.tree.root_node(),
            &parsed.source,
            &parsed.path.display().to_string(),
            &mut units,
        );
    }
    units
}

fn unit_metrics_from_py_function(file: &str, name: &str, line: usize, m: &FunctionMetrics) -> UnitMetrics {
    UnitMetrics {
        file: file.to_string(),
        name: name.to_string(),
        kind: "function",
        line,
        statements: Some(m.statements),
        arguments: Some(m.arguments),
        args_positional: Some(m.arguments_positional),
        args_keyword_only: Some(m.arguments_keyword_only),
        indentation: Some(m.max_indentation),
        nested_depth: Some(m.nested_function_depth),
        branches: Some(m.branches),
        returns: Some(m.returns),
        return_values: Some(m.max_return_values),
        locals: Some(m.local_variables),
        methods: None,
        lines: None,
        imports: None,
        fan_in: None,
        fan_out: None,
        indirect_deps: None,
        dependency_depth: None,
    }
}

fn walk_detailed_children(node: Node, source: &str, file: &str, units: &mut Vec<UnitMetrics>) {
    let mut c = node.walk();
    for child in node.children(&mut c) {
        collect_detailed_from_node(child, source, file, units);
    }
}

/// Returns `true` if children were already walked (parse-error function body).
fn push_py_function_unit(node: Node, source: &str, file: &str, units: &mut Vec<UnitMetrics>) -> bool {
    let name = node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .unwrap_or("?");
    let m = compute_function_metrics(node, source);
    if m.has_error {
        walk_detailed_children(node, source, file, units);
        return true;
    }
    let line = node.start_position().row + 1;
    units.push(unit_metrics_from_py_function(file, name, line, &m));
    false
}

fn push_py_class_unit(node: Node, source: &str, file: &str, units: &mut Vec<UnitMetrics>) {
    let name = node
        .child_by_field_name("name")
        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
        .unwrap_or("?");
    let m = compute_class_metrics(node);
    units.push(UnitMetrics {
        file: file.to_string(),
        name: name.to_string(),
        kind: "class",
        line: node.start_position().row + 1,
        statements: None,
        arguments: None,
        args_positional: None,
        args_keyword_only: None,
        indentation: None,
        nested_depth: None,
        branches: None,
        returns: None,
        return_values: None,
        locals: None,
        methods: Some(m.methods),
        lines: None,
        imports: None,
        fan_in: None,
        fan_out: None,
        indirect_deps: None,
        dependency_depth: None,
    });
}

fn collect_detailed_from_node(node: Node, source: &str, file: &str, units: &mut Vec<UnitMetrics>) {
    let skip_walk = match node.kind() {
        "function_definition" | "async_function_definition" => {
            push_py_function_unit(node, source, file, units)
        }
        "class_definition" => {
            push_py_class_unit(node, source, file, units);
            false
        }
        _ => false,
    };
    if !skip_walk {
        walk_detailed_children(node, source, file, units);
    }
}

#[cfg(test)]
pub(crate) fn collect_detailed_from_node_for_test(
    node: Node,
    source: &str,
    file: &str,
    units: &mut Vec<UnitMetrics>,
) {
    collect_detailed_from_node(node, source, file, units);
}
