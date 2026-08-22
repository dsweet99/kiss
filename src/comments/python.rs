use crate::code_roles::SourceRoleIndex;
use crate::parsing::ParsedFile;
use crate::violation::Violation;
use tree_sitter::{Node, TreeCursor};

use super::{comment_violation, doc_violation, skip_test_only_line};

pub(super) fn append_python_comment_violations(
    parsed: &ParsedFile,
    roles: Option<&SourceRoleIndex>,
    out: &mut Vec<Violation>,
) {
    let mut cursor = parsed.tree.walk();
    walk_comment_nodes(&mut cursor, parsed, roles, out);
}

fn walk_comment_nodes(
    cursor: &mut TreeCursor<'_>,
    parsed: &ParsedFile,
    roles: Option<&SourceRoleIndex>,
    out: &mut Vec<Violation>,
) {
    loop {
        let node = cursor.node();
        if is_comment_kind(node.kind()) {
            let line = node.start_position().row + 1;
            if !skip_test_only_line(roles, &parsed.path, line) {
                out.push(comment_violation(&parsed.path, line));
            }
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return;
            }
        }
    }
}

fn is_comment_kind(kind: &str) -> bool {
    kind == "comment" || kind == "type_comment"
}

pub(super) fn append_python_doc_violations(
    parsed: &ParsedFile,
    roles: Option<&SourceRoleIndex>,
    out: &mut Vec<Violation>,
) {
    let root = parsed.tree.root_node();
    collect_docs_in_body(root, parsed, roles, out);
    visit_defs(root, parsed, roles, out);
}

fn visit_defs(
    node: Node<'_>,
    parsed: &ParsedFile,
    roles: Option<&SourceRoleIndex>,
    out: &mut Vec<Violation>,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "function_definition" | "async_function_definition" | "class_definition" => {
                if let Some(block) = child.child_by_field_name("body") {
                    collect_docs_in_body(block, parsed, roles, out);
                    visit_defs(block, parsed, roles, out);
                }
            }
            "decorated_definition" => visit_defs(child, parsed, roles, out),
            _ => {}
        }
    }
}

fn collect_docs_in_body(
    body: Node<'_>,
    parsed: &ParsedFile,
    roles: Option<&SourceRoleIndex>,
    out: &mut Vec<Violation>,
) {
    let mut cursor = body.walk();
    for child in body.named_children(&mut cursor) {
        if child.kind() == "comment" {
            continue;
        }
        if is_docstring_statement(child) {
            let line = child.start_position().row + 1;
            if !skip_test_only_line(roles, &parsed.path, line) {
                out.push(doc_violation(&parsed.path, line));
            }
        }
    }
}

fn is_docstring_statement(node: Node<'_>) -> bool {
    if node.kind() != "expression_statement" {
        return false;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .next()
        .is_some_and(|n| n.kind() == "string" || n.kind() == "concatenated_string")
}
