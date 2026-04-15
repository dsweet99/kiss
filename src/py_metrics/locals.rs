use std::collections::HashSet;
use tree_sitter::Node;

pub(super) fn update_local_vars(node: Node, source: &str, vars: &mut HashSet<String>) {
    if (node.kind() == "assignment" || node.kind() == "augmented_assignment")
        && let Some(left) = node.child_by_field_name("left")
    {
        collect_assigned_names(left, source, vars);
    }
}

pub(crate) fn collect_assigned_names(node: Node, source: &str, vars: &mut HashSet<String>) {
    match node.kind() {
        "identifier" => {
            if let Ok(name) = node.utf8_text(source.as_bytes()) {
                vars.insert(name.to_string());
            }
        }
        "pattern_list" | "tuple_pattern" => {
            let mut c = node.walk();
            for child in node.children(&mut c) {
                collect_assigned_names(child, source, vars);
            }
        }
        _ => {}
    }
}
