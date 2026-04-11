use tree_sitter::Node;

use super::statements::count_statements;

pub(crate) fn count_file_statements(node: Node) -> usize {
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

pub(crate) fn count_class_statements(body: Node) -> usize {
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
