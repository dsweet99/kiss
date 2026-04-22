use super::compute::{compute_class_metrics, compute_function_metrics};
use super::types::{ClassMetrics, FunctionMetrics};
use tree_sitter::Node;

pub(crate) struct FunctionVisit<'a> {
    pub(crate) metrics: &'a FunctionMetrics,
    pub(crate) name: &'a str,
    pub(crate) line: usize,
    pub(crate) inside_class: bool,
}

pub(crate) struct ClassVisit<'a> {
    pub(crate) metrics: &'a ClassMetrics,
    pub(crate) name: &'a str,
    pub(crate) line: usize,
}

pub(crate) enum PyWalkAction<'a> {
    Function(FunctionVisit<'a>),
    Class(ClassVisit<'a>),
}

pub(crate) fn walk_py_ast(
    node: Node<'_>,
    source: &str,
    sink: &mut impl FnMut(PyWalkAction<'_>),
    inside_class: bool,
) {
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            let name = node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .unwrap_or("<anonymous>");
            let line = node.start_position().row + 1;
            let m = compute_function_metrics(node, source);
            if !m.has_error {
                sink(PyWalkAction::Function(FunctionVisit {
                    metrics: &m,
                    name,
                    line,
                    inside_class,
                }));
            }
            let mut c = node.walk();
            for child in node.children(&mut c) {
                walk_py_ast(child, source, sink, false);
            }
        }
        "class_definition" => {
            let name = node
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                .unwrap_or("<anonymous>");
            let line = node.start_position().row + 1;
            let m = compute_class_metrics(node);
            sink(PyWalkAction::Class(ClassVisit {
                metrics: &m,
                name,
                line,
            }));
            if let Some(body) = node.child_by_field_name("body") {
                let mut c = body.walk();
                for child in body.children(&mut c) {
                    walk_py_ast(child, source, sink, true);
                }
            }
        }
        _ => {
            let mut c = node.walk();
            for child in node.children(&mut c) {
                walk_py_ast(child, source, sink, inside_class);
            }
        }
    }
}
