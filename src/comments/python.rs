use crate::parsing::ParsedFile;
use crate::violation::Violation;
use tree_sitter::{Node, TreeCursor};

use super::{comment_violation, doc_violation};

pub(super) fn append_python_comment_violations(parsed: &ParsedFile, out: &mut Vec<Violation>) {
    let mut cursor = parsed.tree.walk();
    walk_comment_nodes(&mut cursor, parsed, out);
}

fn walk_comment_nodes(cursor: &mut TreeCursor<'_>, parsed: &ParsedFile, out: &mut Vec<Violation>) {
    loop {
        let node = cursor.node();
        if is_comment_kind(node.kind()) {
            out.push(comment_violation(
                &parsed.path,
                node.start_position().row + 1,
            ));
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

pub(super) fn append_python_doc_violations(parsed: &ParsedFile, out: &mut Vec<Violation>) {
    let root = parsed.tree.root_node();
    collect_docs_in_body(root, parsed, out);
    visit_defs(root, parsed, out);
}

fn visit_defs(node: Node<'_>, parsed: &ParsedFile, out: &mut Vec<Violation>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "function_definition" | "async_function_definition" | "class_definition" => {
                if let Some(block) = child.child_by_field_name("body") {
                    collect_docs_in_body(block, parsed, out);
                    visit_defs(block, parsed, out);
                }
            }
            "decorated_definition" => visit_defs(child, parsed, out),
            _ => {}
        }
    }
}

fn collect_docs_in_body(body: Node<'_>, parsed: &ParsedFile, out: &mut Vec<Violation>) {
    let mut cursor = body.walk();
    for child in body.named_children(&mut cursor) {
        if child.kind() == "comment" {
            continue;
        }
        if is_docstring_statement(child) {
            out.push(doc_violation(&parsed.path, child.start_position().row + 1));
        }
        return;
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
