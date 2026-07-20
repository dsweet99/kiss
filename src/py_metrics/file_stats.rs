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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::parse_python_source;

    #[test]
    fn count_file_statements_accumulates_function_class_and_decorated_bodies() {
        let parsed = parse_python_source(
            "def f():\n    x = 1\n\n@decorator\nclass C:\n    def m(self):\n        y = 2\n",
        );

        assert_eq!(count_file_statements(parsed.tree.root_node()), 2);
    }

    #[test]
    fn count_file_statements_sums_multiple_top_level_bodies() {
        let parsed = parse_python_source(
            "async def af():\n    x = 1\n    return x\n\n\
             def f():\n    y = 2\n\n\
             class C:\n    def m(self):\n        z = 3\n        return z\n",
        );

        assert_eq!(count_file_statements(parsed.tree.root_node()), 5);
    }

    #[test]
    fn count_class_statements_accumulates_decorated_method_body() {
        let parsed = parse_python_source(
            "class C:\n    @decorator\n    def m(self):\n        y = 2\n        return y\n",
        );
        let class_body = parsed
            .tree
            .root_node()
            .child(0)
            .expect("class")
            .child_by_field_name("body")
            .expect("class body");

        assert_eq!(count_class_statements(class_body), 2);
    }

    #[test]
    fn count_file_statements_handles_async_and_decorated_functions() {
        let parsed = parse_python_source(
            "@decorator\nasync def f():\n    x = 1\n    return x\n\nclass C:\n    def m(self):\n        y = 2\n",
        );

        assert_eq!(count_file_statements(parsed.tree.root_node()), 3);
    }

    #[test]
    fn count_class_statements_ignores_non_method_class_body_items() {
        let parsed =
            parse_python_source("class C:\n    value = 1\n    def m(self):\n        y = 2\n");
        let class_body = parsed
            .tree
            .root_node()
            .child(0)
            .expect("class")
            .child_by_field_name("body")
            .expect("class body");

        assert_eq!(count_class_statements(class_body), 1);
    }

    #[test]
    fn statement_counters_recurse_through_decorated_async_class_members() {
        let parsed = parse_python_source(
            "@outer\nclass C:\n    value = 1\n    @inner\n    async def m(self):\n        y = 2\n        return y\n\n@decorator\nasync def f():\n    z = 3\n",
        );
        fn find_class(node: Node<'_>) -> Option<Node<'_>> {
            if node.kind() == "class_definition" {
                return Some(node);
            }
            let mut cursor = node.walk();
            node.children(&mut cursor).find_map(find_class)
        }
        let root = parsed.tree.root_node();
        let class_body = find_class(root)
            .expect("class")
            .child_by_field_name("body")
            .expect("class body");

        assert_eq!(count_class_statements(class_body), 2);
        assert_eq!(count_file_statements(parsed.tree.root_node()), 3);
    }
}
