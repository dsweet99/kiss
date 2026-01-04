
use std::collections::HashSet;
use tree_sitter::Node;

pub fn count_imports(node: Node, source: &str) -> usize {
    let mut names = HashSet::new();
    collect_import_names(node, source, &mut names);
    names.len()
}

fn collect_import_names(node: Node, source: &str, names: &mut HashSet<String>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_statement" => collect_import_statement_names(child, source, names),
            "import_from_statement" => collect_import_from_names(child, source, names),
            _ => collect_import_names(child, source, names),
        }
    }
}

fn collect_import_statement_names(node: Node, source: &str, names: &mut HashSet<String>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "dotted_name" => {
                // `import torch.nn` binds `torch` locally
                if let Some(first) = child.child(0)
                    && let Ok(name) = first.utf8_text(source.as_bytes())
                {
                    names.insert(name.to_string());
                }
            }
            "aliased_import" => {
                // `import foo as bar` binds `bar`
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

fn collect_import_from_names(node: Node, source: &str, names: &mut HashSet<String>) {
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
        // Unique counting: duplicate imports count as 1
        let p3 = parse("def f():\n    import torch\ndef g():\n    import torch");
        assert_eq!(count_imports(p3.tree.root_node(), &p3.source), 1);
    }
}

