use std::collections::HashSet;
use tree_sitter::Node;

pub fn count_imports(node: Node, source: &str) -> usize {
    let mut names = HashSet::new();
    collect_import_names(node, source, &mut names);
    names.len()
}

pub fn is_type_checking_block(node: Node, source: &str) -> bool {
    if node.kind() != "if_statement" {
        return false;
    }
    let Some(condition) = node.child_by_field_name("condition") else {
        return false;
    };
    match condition.kind() {
        "identifier" => condition
            .utf8_text(source.as_bytes())
            .is_ok_and(|s| s == "TYPE_CHECKING"),
        "attribute" => condition
            .child_by_field_name("attribute")
            .is_some_and(|attr| {
                attr.utf8_text(source.as_bytes())
                    .is_ok_and(|s| s == "TYPE_CHECKING")
            }),
        _ => false,
    }
}

fn collect_import_names(node: Node, source: &str, names: &mut HashSet<String>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if is_type_checking_block(child, source) {
            continue;
        }
        match child.kind() {
            "import_statement" => collect_import_statement_names(child, source, names),
            "import_from_statement" => collect_import_from_names(child, source, names),
            _ => collect_import_names(child, source, names),
        }
    }
}

pub(crate) fn collect_import_statement_names(node: Node, source: &str, names: &mut HashSet<String>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "dotted_name" => {
                if let Some(first) = child.child(0)
                    && let Ok(name) = first.utf8_text(source.as_bytes())
                {
                    names.insert(name.to_string());
                }
            }
            "aliased_import" => {
                if let Some(alias) = child.child_by_field_name("alias")
                    && let Ok(name) = alias.utf8_text(source.as_bytes())
                {
                    names.insert(name.to_string());
                } else if let Some(name_node) = child.child_by_field_name("name")
                    && let Some(first) = name_node.child(0)
                    && let Ok(name) = first.utf8_text(source.as_bytes())
                {
                    names.insert(name.to_string());
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn collect_import_from_names(node: Node, source: &str, names: &mut HashSet<String>) {
    let mut seen_import = false;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import" => seen_import = true,
            "dotted_name" if seen_import => {
                if let Ok(name) = child.utf8_text(source.as_bytes()) {
                    names.insert(name.to_string());
                }
            }
            "aliased_import" if seen_import => {
                if let Some(alias) = child.child_by_field_name("alias")
                    && let Ok(name) = alias.utf8_text(source.as_bytes())
                {
                    names.insert(name.to_string());
                } else if let Some(name_node) = child.child_by_field_name("name")
                    && let Ok(name) = name_node.utf8_text(source.as_bytes())
                {
                    names.insert(name.to_string());
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::parse_python_source as parse;

    #[test]
    fn test_import_counting() {
        let p1 = parse("import os");
        assert_eq!(count_imports(p1.tree.root_node(), &p1.source), 1);
        let p2 = parse("from typing import Any, List");
        assert_eq!(count_imports(p2.tree.root_node(), &p2.source), 2);
        let p3 = parse("def f():\n    import torch\ndef g():\n    import torch");
        assert_eq!(count_imports(p3.tree.root_node(), &p3.source), 1);
    }

    #[test]
    fn test_type_checking_imports_excluded() {
        let code = "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from some_module import SomeClass\nimport os";
        let p = parse(code);
        assert_eq!(count_imports(p.tree.root_node(), &p.source), 2);

        let code2 = "import typing\nif typing.TYPE_CHECKING:\n    from foo import Bar";
        let p2 = parse(code2);
        assert_eq!(count_imports(p2.tree.root_node(), &p2.source), 1);
    }

    #[test]
    fn test_is_type_checking_block_direct() {
        let p = parse("if TYPE_CHECKING:\n    import os\nx = 1");
        // root child 0 is the if statement
        let if_node = p.tree.root_node().child(0).unwrap();
        assert!(is_type_checking_block(if_node, &p.source));
    }
}
