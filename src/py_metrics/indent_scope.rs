pub(super) fn next_indent_depth(kind: &str, current_depth: usize) -> usize {
    if is_indent_increasing(kind) {
        current_depth + 1
    } else {
        current_depth
    }
}

pub(super) fn is_indent_increasing(kind: &str) -> bool {
    matches!(
        kind,
        "if_statement"
            | "for_statement"
            | "while_statement"
            | "try_statement"
            | "with_statement"
            | "match_statement"
            | "function_definition"
            | "class_definition"
            | "async_function_definition"
            | "async_for_statement"
            | "async_with_statement"
            | "elif_clause"
            | "else_clause"
            | "except_clause"
            | "finally_clause"
            | "case_clause"
    )
}

pub(super) fn is_nested_scope_boundary(kind: &str) -> bool {
    matches!(
        kind,
        "function_definition"
            | "async_function_definition"
            | "class_definition"
            | "decorated_definition"
    )
}
