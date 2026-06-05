use tree_sitter::Node;

mod eval;

pub(crate) fn is_py_const_false(node: Node, source: &str) -> bool {
    match node.kind() {
        "false" | "none" => true,
        "integer" => {
            let text = &source[node.start_byte()..node.end_byte()];
            text == "0"
        }
        "unary_operator" => {
            let mut cursor = node.walk();
            let mut op = String::new();
            let mut operand = None;
            for child in node.children(&mut cursor) {
                match child.kind() {
                    "not" | "!" => op = child.kind().to_string(),
                    _ if operand.is_none() => operand = Some(child),
                    _ => {}
                }
            }
            op == "not" && operand.is_some_and(|o| eval::is_py_const_true(o, source))
        }
        "comparison_operator" => eval::eval_py_comparison(node, source) == Some(false),
        "boolean_operator" => eval::eval_py_boolean(node, source),
        _ => false,
    }
}

fn handle_py_if_statement(node: Node, source: &str, visit: &mut impl FnMut(Node)) {
    let cond_false = node
        .child_by_field_name("condition")
        .is_some_and(|c| is_py_const_false(c, source));
    if !cond_false
        && let Some(body) = node.child_by_field_name("consequence")
    {
        collect_py_live_scope(body, source, visit);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "elif_clause" => {
                if child
                    .child_by_field_name("condition")
                    .is_some_and(|c| is_py_const_false(c, source))
                {
                    continue;
                }
                if let Some(body) = child.child_by_field_name("consequence") {
                    collect_py_live_scope(body, source, visit);
                }
            }
            "else_clause" => collect_py_live_scope(child, source, visit),
            _ => {
                if child.kind() == "block"
                    && node.child_by_field_name("alternative") == Some(child)
                {
                    collect_py_live_scope(child, source, visit);
                }
            }
        }
    }
}

pub(crate) fn collect_py_live_scope(node: Node, source: &str, visit: &mut impl FnMut(Node)) {
    if matches!(node.kind(), "import_from_statement" | "import_statement") {
        visit(node);
        return;
    }
    if node.kind() == "if_statement" {
        handle_py_if_statement(node, source, visit);
        return;
    }
    if matches!(node.kind(), "while_statement" | "for_statement" | "async_for_statement")
        && node
            .child_by_field_name("condition")
            .is_some_and(|c| is_py_const_false(c, source))
    {
        return;
    }
    visit(node);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_py_live_scope(child, source, visit);
    }
}

pub(crate) fn count_py_live_branches(node: Node, source: &str) -> usize {
    let mut count = 0usize;
    collect_py_live_scope(node, source, &mut |n| {
        if matches!(
            n.kind(),
            "if_statement" | "elif_clause" | "case_clause" | "for_statement" | "while_statement"
                | "async_for_statement"
        ) {
            count += 1;
        }
    });
    count
}

#[cfg(test)]
mod inline_tests;
