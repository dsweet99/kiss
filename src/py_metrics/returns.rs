use tree_sitter::Node;

pub(super) fn count_return_values(node: Node) -> usize {
    // return statement child is the expression being returned
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "return" {
            continue;
        } // skip the 'return' keyword
        return match child.kind() {
            "expression_list" | "tuple" => child.named_child_count(),
            "parenthesized_expression" => child
                .named_child(0)
                .filter(|inner| matches!(inner.kind(), "expression_list" | "tuple"))
                .map_or(1, |inner| inner.named_child_count()),
            _ => 1, // single value
        };
    }
    0 // bare return
}
