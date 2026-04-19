use crate::graph::DependencyGraph;
use crate::parsing::ParsedFile;
use crate::py_metrics::{
    compute_class_metrics, compute_file_metrics, compute_function_metrics, FunctionMetrics,
};
use tree_sitter::Node;

use super::types::UnitMetrics;
use super::{FileScopeMetrics, file_unit_metrics};

pub fn collect_detailed_py(
    parsed_files: &[&ParsedFile],
    graph: Option<&DependencyGraph>,
) -> Vec<UnitMetrics> {
    let mut units = Vec::new();
    for parsed in parsed_files {
        let fm = compute_file_metrics(parsed);
        let lines = parsed.source.lines().count();
        units.push(file_unit_metrics(
            &parsed.path,
            FileScopeMetrics {
                lines,
                imports: fm.imports,
                statements: fm.statements,
                functions: fm.functions,
                interface_types: fm.interface_types,
                concrete_types: fm.concrete_types,
            },
            graph,
        ));
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
    let mut u = UnitMetrics::new(file.to_string(), name.to_string(), "function", line);
    u.statements = Some(m.statements);
    u.arguments = Some(m.arguments);
    u.args_positional = Some(m.arguments_positional);
    u.args_keyword_only = Some(m.arguments_keyword_only);
    u.indentation = Some(m.max_indentation);
    u.nested_depth = Some(m.nested_function_depth);
    u.branches = Some(m.branches);
    u.returns = Some(m.returns);
    u.return_values = Some(m.max_return_values);
    u.locals = Some(m.local_variables);
    u.try_block_statements = Some(m.max_try_block_statements);
    u.boolean_parameters = Some(m.boolean_parameters);
    u.annotations = Some(m.decorators);
    u.calls = Some(m.calls);
    u
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
    let mut u = UnitMetrics::new(
        file.to_string(),
        name.to_string(),
        "class",
        node.start_position().row + 1,
    );
    u.methods = Some(m.methods);
    units.push(u);
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
