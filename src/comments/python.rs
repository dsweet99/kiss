use crate::parsing::ParsedFile;
use crate::violation::Violation;
use tree_sitter::TreeCursor;

use super::comment_violation;

pub(super) fn append_python_comment_violations(parsed: &ParsedFile, out: &mut Vec<Violation>) {
    let mut cursor = parsed.tree.walk();
    walk_comment_nodes(&mut cursor, parsed, out);
}

fn walk_comment_nodes(cursor: &mut TreeCursor<'_>, parsed: &ParsedFile, out: &mut Vec<Violation>) {
    loop {
        let node = cursor.node();
        if is_comment_kind(node.kind()) {
            out.push(comment_violation(&parsed.path, node.start_position().row + 1));
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
