use tree_sitter::Node;

use super::statements::count_statements;

fn count_body_with(node: Node, count: fn(Node) -> usize) -> usize {
    node.child_by_field_name("body").map(count).unwrap_or(0)
}

pub(crate) fn count_file_statements(node: Node) -> usize {
    let mut total = 0;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_definition" | "async_function_definition" => {
                total += count_body_with(child, count_statements);
            }
            "class_definition" => {
                total += count_body_with(child, count_class_statements);
            }
            "decorated_definition" => {
                total += count_file_statements(child);
            }
            _ => {}
        }
    }
    total
}

pub(crate) fn count_class_statements(body: Node) -> usize {
    let mut total = 0;
    let mut cursor = body.walk();
    for child in body.children(&mut cursor) {
        match child.kind() {
            "function_definition" | "async_function_definition" => {
                total += count_body_with(child, count_statements);
            }
            "decorated_definition" => {
                total += count_class_statements(child);
            }
            _ => {}
        }
    }
    total
}
