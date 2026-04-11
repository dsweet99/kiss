use tree_sitter::Node;

pub(crate) fn compute_nested_function_depth(node: Node, current_depth: usize) -> usize {
    let is_fn = matches!(
        node.kind(),
        "function_definition" | "async_function_definition"
    );
    let new_depth = if is_fn {
        current_depth + 1
    } else {
        current_depth
    };
    let mut cursor = node.walk();
    let max = node.children(&mut cursor).fold(new_depth, |m, c| {
        m.max(compute_nested_function_depth(c, new_depth))
    });
    if is_fn && current_depth == 0 {
        max.saturating_sub(1)
    } else {
        max
    }
}

pub fn count_node_kind(node: Node, kind: &str) -> usize {
    let mut cursor = node.walk();
    usize::from(node.kind() == kind)
        + node
            .children(&mut cursor)
            .map(|c| count_node_kind(c, kind))
            .sum::<usize>()
}
