use tree_sitter::Node;

pub(crate) fn collect_py_scope(node: Node, _source: &str, visit: &mut impl FnMut(Node)) {
    if matches!(node.kind(), "import_from_statement" | "import_statement") {
        visit(node);
        return;
    }
    visit(node);
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_py_scope(child, _source, visit);
    }
}

pub(crate) fn count_py_branches(node: Node) -> usize {
    let mut count = 0usize;
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        if matches!(
            n.kind(),
            "if_statement"
                | "elif_clause"
                | "case_clause"
                | "for_statement"
                | "while_statement"
                | "async_for_statement"
        ) {
            count += 1;
        }
        let mut cursor = n.walk();
        for child in n.children(&mut cursor) {
            stack.push(child);
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_expr(src: &str) -> (tree_sitter::Tree, String) {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(src, None).unwrap();
        (tree, src.to_string())
    }

    #[test]
    fn collect_py_scope_visits_all_nodes_including_dead_if() {
        let src = "def test_fn():\n    if False:\n        foo()\n    bar()\n";
        let (tree, src) = parse_expr(src);
        let body = tree
            .root_node()
            .named_child(0)
            .unwrap()
            .child_by_field_name("body")
            .unwrap();
        let mut refs = std::collections::HashSet::new();
        collect_py_scope(body, &src, &mut |n| {
            if n.kind() == "identifier" {
                let name = &src[n.start_byte()..n.end_byte()];
                if name != "test_fn" {
                    refs.insert(name.to_string());
                }
            }
        });
        assert!(refs.contains("bar"));
        assert!(refs.contains("foo"));
    }

    #[test]
    fn import_names_do_not_recurse_into_usage() {
        let src = "from mod import fn\n";
        let (tree, src) = parse_expr(src);
        let mut usage = std::collections::HashSet::new();
        collect_py_scope(tree.root_node(), &src, &mut |n| {
            if n.kind() == "identifier" {
                usage.insert(src[n.start_byte()..n.end_byte()].to_string());
            }
        });
        assert!(!usage.contains("fn"));
    }

    #[test]
    fn count_py_branches_counts_syntactic_branches() {
        let src = "def test_x():\n    if False:\n        pass\n    for x in items:\n        pass\n";
        let (tree, _src) = parse_expr(src);
        let func = tree.root_node().named_child(0).unwrap();
        let n = count_py_branches(func);
        assert_eq!(n, 2);
    }
}
