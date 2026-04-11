use crate::py_imports::{
    collect_import_from_names, collect_import_statement_names, is_type_checking_block,
};
use std::collections::HashSet;
use tree_sitter::Node;

#[derive(Default)]
pub(crate) struct FileCounts {
    pub(crate) interface_types: usize,
    pub(crate) concrete_types: usize,
    pub(crate) functions: usize,
    pub(crate) import_names: HashSet<String>,
}

pub(crate) fn collect_file_counts(root: Node, source: &str) -> FileCounts {
    let mut agg = FileCounts::default();
    walk_file(root, source, false, &mut agg);
    agg
}

pub(crate) fn walk_file(
    node: Node,
    source: &str,
    in_type_checking: bool,
    agg: &mut FileCounts,
) {
    let now_type_checking = in_type_checking || is_type_checking_block(node, source);

    match node.kind() {
        "function_definition" | "async_function_definition" => agg.functions += 1,
        "class_definition" => {
            if is_interface_type(node, source) {
                agg.interface_types += 1;
            } else {
                agg.concrete_types += 1;
            }
        }
        "import_statement" if !now_type_checking => {
            collect_import_statement_names(node, source, &mut agg.import_names);
        }
        "import_from_statement" if !now_type_checking => {
            collect_import_from_names(node, source, &mut agg.import_names);
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_file(child, source, now_type_checking, agg);
    }
}

pub(crate) fn is_interface_type(class_node: Node, source: &str) -> bool {
    // tree-sitter-python has historically used "superclasses" as a field name, but to be robust we
    // also fall back to finding an argument_list child directly.
    let supers = class_node.child_by_field_name("superclasses").or_else(|| {
        let mut c = class_node.walk();
        class_node
            .children(&mut c)
            .find(|n| n.kind() == "argument_list")
    });

    let Some(supers) = supers else {
        return false;
    };
    let Ok(text) = supers.utf8_text(source.as_bytes()) else {
        return false;
    };

    let mut token = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch);
        } else {
            if is_interface_token(&token) {
                return true;
            }
            token.clear();
        }
    }
    is_interface_token(&token)
}

pub(crate) fn is_interface_token(token: &str) -> bool {
    matches!(token, "Protocol" | "ABC" | "ABCMeta")
}
