use super::detection::{is_abstract_method, is_protocol_class, is_python_test_file};
use super::{CodeDefinition, PerTestUsage};
use crate::parsing::ParsedFile;
use crate::units::{CodeUnitKind, get_child_by_field};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tree_sitter::Node;

pub(crate) fn try_add_def(
    node: Node,
    source: &str,
    file: &Path,
    defs: &mut Vec<CodeDefinition>,
    kind: CodeUnitKind,
    containing_class: Option<String>,
) {
    if let Some(name) = get_child_by_field(node, "name", source)
        && (!name.starts_with('_') || name == "__init__")
        && !name.starts_with("test_")
    {
        defs.push(CodeDefinition {
            name,
            kind,
            file: file.to_path_buf(),
            line: node.start_position().row + 1,
            containing_class,
        });
    }
}

pub(crate) fn collect_definitions(
    node: Node,
    source: &str,
    file: &Path,
    defs: &mut Vec<CodeDefinition>,
    inside_class: bool,
    class_name: Option<&str>,
) {
    match node.kind() {
        "function_definition" | "async_function_definition" if is_abstract_method(node, source) => {
        }
        "function_definition" | "async_function_definition" => {
            let kind = if inside_class {
                CodeUnitKind::Method
            } else {
                CodeUnitKind::Function
            };
            try_add_def(node, source, file, defs, kind, class_name.map(String::from));
        }
        "class_definition" if is_protocol_class(node, source) => {}
        "class_definition" => {
            try_add_def(node, source, file, defs, CodeUnitKind::Class, None);
            let name = get_child_by_field(node, "name", source);
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_definitions(child, source, file, defs, true, name.as_deref());
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_definitions(child, source, file, defs, inside_class, class_name);
            }
        }
    }
}

pub(crate) fn insert_identifier(node: Node, source: &str, refs: &mut HashSet<String>) {
    refs.insert(source[node.start_byte()..node.end_byte()].to_string());
}

pub(crate) fn collect_usage_refs_in_scope(node: Node, source: &str, refs: &mut HashSet<String>) {
    match node.kind() {
        "call" => {
            if let Some(func) = node.child_by_field_name("function") {
                collect_call_target(func, source, refs);
            }
        }
        "type" => {
            collect_type_refs(node, source, refs);
        }
        "decorator" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "identifier"
                    || child.kind() == "attribute"
                    || child.kind() == "call"
                {
                    collect_call_target(child, source, refs);
                }
            }
        }
        "identifier" => {
            insert_identifier(node, source, refs);
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_usage_refs_in_scope(child, source, refs);
    }
}

pub(crate) fn collect_class_test_methods(
    class_body: Node,
    source: &str,
    class_prefix: &str,
    out: &mut Vec<(String, HashSet<String>)>,
) {
    let mut cursor = class_body.walk();
    for child in class_body.children(&mut cursor) {
        if child.kind() != "function_definition" && child.kind() != "async_function_definition" {
            continue;
        }
        let meth_name = get_child_by_field(child, "name", source).unwrap_or_default();
        if !meth_name.starts_with("test_") {
            continue;
        }
        let mut refs = HashSet::new();
        if let Some(meth_body) = child.child_by_field_name("body") {
            collect_usage_refs_in_scope(meth_body, source, &mut refs);
        }
        let test_id = format!("{class_prefix}::{meth_name}");
        out.push((test_id, refs));
    }
}

pub(crate) fn collect_test_functions_with_refs(
    node: Node,
    source: &str,
    prefix: &str,
    out: &mut Vec<(String, HashSet<String>)>,
) {
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            let name = get_child_by_field(node, "name", source).unwrap_or_default();
            if name.starts_with("test_") {
                let mut refs = HashSet::new();
                if let Some(body) = node.child_by_field_name("body") {
                    collect_usage_refs_in_scope(body, source, &mut refs);
                }
                let test_id = if prefix.is_empty() {
                    name
                } else {
                    format!("{prefix}::{name}")
                };
                out.push((test_id, refs));
            }
        }
        "class_definition" => {
            let class_name = get_child_by_field(node, "name", source).unwrap_or_default();
            if class_name.starts_with("Test") {
                let class_prefix = if prefix.is_empty() {
                    class_name
                } else {
                    format!("{prefix}::{class_name}")
                };
                if let Some(body) = node.child_by_field_name("body") {
                    collect_class_test_methods(body, source, &class_prefix, out);
                }
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_test_functions_with_refs(child, source, prefix, out);
            }
        }
    }
}

pub(crate) fn collect_all_test_file_data(
    node: Node,
    source: &str,
    test_refs: &mut HashSet<String>,
    usage_refs: &mut HashSet<String>,
    import_bindings: &mut HashMap<String, HashSet<String>>,
) {
    match node.kind() {
        "call" => {
            if let Some(func) = node.child_by_field_name("function") {
                collect_call_target(func, source, test_refs);
                collect_call_target(func, source, usage_refs);
            }
        }
        "import_from_statement" => {
            collect_import_names(node, source, test_refs);
            extract_import_from_binding(node, source, import_bindings);
            return;
        }
        "import_statement" => {
            collect_import_names(node, source, test_refs);
            return;
        }
        "type" => {
            collect_type_refs(node, source, test_refs);
            collect_type_refs(node, source, usage_refs);
            return;
        }
        "decorator" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "identifier"
                    || child.kind() == "attribute"
                    || child.kind() == "call"
                {
                    collect_call_target(child, source, test_refs);
                    collect_call_target(child, source, usage_refs);
                }
            }
        }
        "identifier" => {
            insert_identifier(node, source, usage_refs);
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_all_test_file_data(child, source, test_refs, usage_refs, import_bindings);
    }
}

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

pub(crate) fn collect_import_names(node: Node, source: &str, refs: &mut HashSet<String>) {
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

type CollectedRefs = (
    Vec<CodeDefinition>,
    HashSet<String>,
    HashSet<String>,
    HashMap<String, HashSet<String>>,
    PerTestUsage,
);

fn empty_collected() -> CollectedRefs {
    (
        Vec::new(),
        HashSet::new(),
        HashSet::new(),
        HashMap::new(),
        PerTestUsage::new(),
    )
}

fn merge_collected(
    (mut defs, mut t_refs, mut u_refs, mut i_binds, mut pt): CollectedRefs,
    (defs2, t_refs2, u_refs2, i_binds2, pt2): CollectedRefs,
) -> CollectedRefs {
    defs.extend(defs2);
    t_refs.extend(t_refs2);
    u_refs.extend(u_refs2);
    for (module, names) in i_binds2 {
        i_binds.entry(module).or_default().extend(names);
    }
    pt.extend(pt2);
    (defs, t_refs, u_refs, i_binds, pt)
}

pub(crate) fn collect_refs_parallel(
    parsed_files: &[&ParsedFile],
    need_coverage_map: bool,
) -> CollectedRefs {
    parsed_files
        .par_iter()
        .map(|parsed| {
            let mut r = empty_collected();
            if is_python_test_file(parsed) {
                collect_all_test_file_data(
                    parsed.tree.root_node(),
                    &parsed.source,
                    &mut r.1,
                    &mut r.2,
                    &mut r.3,
                );
                if need_coverage_map {
                    let mut test_funcs = Vec::new();
                    collect_test_functions_with_refs(
                        parsed.tree.root_node(),
                        &parsed.source,
                        "",
                        &mut test_funcs,
                    );
                    r.4 = vec![(parsed.path.clone(), test_funcs)];
                }
            } else {
                collect_definitions(
                    parsed.tree.root_node(),
                    &parsed.source,
                    &parsed.path,
                    &mut r.0,
                    false,
                    None,
                );
            }
            r
        })
        .fold(empty_collected, merge_collected)
        .reduce(empty_collected, merge_collected)
}
