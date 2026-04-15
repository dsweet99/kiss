use tree_sitter::Node;

use super::indent_scope::is_nested_scope_boundary;

pub(super) fn count_statements(node: Node) -> usize {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .map(|c| {
            let stmt = usize::from(is_statement(c.kind()));
            if is_nested_scope_boundary(c.kind()) {
                stmt
            } else {
                stmt + count_statements(c)
            }
        })
        .sum()
}

pub(super) fn is_statement(kind: &str) -> bool {
    // Statement definition: any statement within a function body that is not an import or signature.
    // Excludes: import_statement, import_from_statement, future_import_statement
    matches!(
        kind,
        "expression_statement"
            | "return_statement"
            | "pass_statement"
            | "break_statement"
            | "continue_statement"
            | "raise_statement"
            | "assert_statement"
            | "global_statement"
            | "nonlocal_statement"
            | "if_statement"
            | "for_statement"
            | "while_statement"
            | "try_statement"
            | "with_statement"
            | "match_statement"
            | "async_for_statement"
            | "async_with_statement"
            | "delete_statement"
            | "exec_statement"
            | "print_statement"
            | "type_alias_statement"
    )
}
