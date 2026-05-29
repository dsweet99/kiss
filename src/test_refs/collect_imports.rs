use super::insert_identifier;
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub(crate) fn collect_type_refs(node: Node, source: &str, refs: &mut HashSet<String>) {
    match node.kind() {
        "identifier" => insert_identifier(node, source, refs),
        "attribute" => {
            if let Some(attr) = node.child_by_field_name("attribute") {
                insert_identifier(attr, source, refs);
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_type_refs(child, source, refs);
            }
        }
    }
}

pub(crate) fn collect_call_target(node: Node, source: &str, refs: &mut HashSet<String>) {
    match node.kind() {
        "identifier" => insert_identifier(node, source, refs),
        "attribute" => {
            if let Some(attr) = node.child_by_field_name("attribute") {
                insert_identifier(attr, source, refs);
            }
            if let Some(obj) = node.child_by_field_name("object") {
                collect_call_target(obj, source, refs);
            }
        }
        _ => {}
    }
}

pub(crate) fn call_tail_name<'a>(func: Node, source: &'a str) -> Option<&'a str> {
    match func.kind() {
        "attribute" => func
            .child_by_field_name("attribute")
            .map(|a| &source[a.start_byte()..a.end_byte()]),
        "identifier" => Some(&source[func.start_byte()..func.end_byte()]),
        _ => None,
    }
}

/// `assertRaises` / `pytest.raises` exception type args are witnesses for calibration.
pub(crate) fn collect_raises_type_args(node: Node, source: &str, refs: &mut HashSet<String>) {
    let Some(func) = node.child_by_field_name("function") else {
        return;
    };
    let Some(name) = call_tail_name(func, source) else {
        return;
    };
    if name != "assertRaises" && name != "raises" {
        return;
    }
    let Some(args) = node.child_by_field_name("arguments") else {
        return;
    };
    first_positional_identifier(args, source, refs);
}

pub(crate) fn first_positional_identifier(node: Node, source: &str, refs: &mut HashSet<String>) -> bool {
    if node.kind() == "identifier" {
        insert_identifier(node, source, refs);
        return true;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if first_positional_identifier(child, source, refs) {
            return true;
        }
    }
    false
}

/// `import pkg.mod` — record module paths even when no `from … import` names are bound.
pub(crate) fn extract_import_statement_modules(
    node: Node,
    source: &str,
    bindings: &mut HashMap<String, HashSet<String>>,
) {
    if node.kind() != "import_statement" {
        return;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "dotted_name" || child.kind() == "identifier" {
            let module_path = &source[child.start_byte()..child.end_byte()];
            if !module_path.starts_with('.') {
                bindings.entry(module_path.to_string()).or_default();
            }
        }
    }
}

pub(crate) fn extract_import_from_binding(
    node: Node,
    source: &str,
    bindings: &mut HashMap<String, HashSet<String>>,
) {
    let Some(module_node) = node.child_by_field_name("module_name") else {
        return;
    };
    let module_path = &source[module_node.start_byte()..module_node.end_byte()];
    if module_path.starts_with('.') {
        return;
    }

    let names = bindings.entry(module_path.to_string()).or_default();
    let module_id = module_node.id();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.id() == module_id {
            continue;
        }
        match child.kind() {
            "identifier" => {
                names.insert(source[child.start_byte()..child.end_byte()].to_string());
            }
            "aliased_import" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    names.insert(source[name_node.start_byte()..name_node.end_byte()].to_string());
                }
            }
            "dotted_name" => {
                let text = &source[child.start_byte()..child.end_byte()];
                if let Some(last) = text.rsplit('.').next() {
                    names.insert(last.to_string());
                }
            }
            _ => {}
        }
    }
}

/// Map `from m import foo as bar` alias bindings in test sources to canonical imported names.
pub(crate) fn collect_py_import_alias_map(
    node: Node,
    source: &str,
    out: &mut HashMap<String, String>,
) {
    if node.kind() == "import_from_statement" {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "aliased_import"
                && let (Some(name), Some(alias)) = (
                    child.child_by_field_name("name"),
                    child.child_by_field_name("alias"),
                )
            {
                out.insert(
                    source[alias.start_byte()..alias.end_byte()].to_string(),
                    source[name.start_byte()..name.end_byte()].to_string(),
                );
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_py_import_alias_map(child, source, out);
    }
}

pub(crate) fn expand_witness_refs_via_import_aliases(
    parsed_files: &[&crate::parsing::ParsedFile],
    refs: &mut HashSet<String>,
) {
    use super::super::detection::is_python_test_file;
    let mut alias_map = HashMap::new();
    for parsed in parsed_files {
        if is_python_test_file(parsed) {
            collect_py_import_alias_map(parsed.tree.root_node(), &parsed.source, &mut alias_map);
        }
    }
    let extras: Vec<String> = refs
        .iter()
        .filter_map(|witness| alias_map.get(witness).cloned())
        .collect();
    refs.extend(extras);
}

pub(crate) fn collect_py_import_names_for_refs(node: Node, source: &str, refs: &mut HashSet<String>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "dotted_name" | "aliased_import" => {
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    if inner.kind() == "identifier" {
                        insert_identifier(inner, source, refs);
                    }
                }
            }
            "identifier" => insert_identifier(child, source, refs),
            _ => {}
        }
    }
}
