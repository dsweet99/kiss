use tree_sitter::Node;

use super::ast_models::{Definition, Reference, ReferenceKind};
use super::ast_python::{
    collect_decorator, collect_identifier_children, collect_py_call, collect_py_def,
    collect_py_import, collect_raise_from, handle_decorated, name_text, python_identifier_is_value,
    recurse_py,
};

fn walk_py_definitions(
    node: Node<'_>,
    src: &str,
    owner: Option<&str>,
    inside_fn: bool,
    defs: &mut Vec<Definition>,
    refs: &mut Vec<Reference>,
) -> bool {
    match node.kind() {
        "decorated_definition" => {
            handle_decorated(node, src, owner, inside_fn, defs, refs);
            true
        }
        "function_definition" | "async_function_definition" => {
            collect_py_def(node, node, src, owner, defs);
            recurse_py(node, src, None, true, defs, refs);
            true
        }
        "class_definition" => {
            let name = name_text(node, src);
            collect_py_def(node, node, src, owner, defs);
            if let Some(body) = node.child_by_field_name("body") {
                let mut c = body.walk();
                for child in body.children(&mut c) {
                    walk_py(child, src, name.as_deref(), false, defs, refs);
                }
            }
            true
        }
        _ => false,
    }
}

fn walk_py_calls_and_imports(
    node: Node<'_>,
    src: &str,
    owner: Option<&str>,
    inside_fn: bool,
    defs: &mut Vec<Definition>,
    refs: &mut Vec<Reference>,
) -> bool {
    match node.kind() {
        "call" => {
            collect_py_call(node, src, refs);
            recurse_py(node, src, owner, inside_fn, defs, refs);
            true
        }
        "import_from_statement" | "import_statement" => {
            collect_py_import(node, src, refs);
            true
        }
        "await" => {
            collect_identifier_children(node, refs);
            recurse_py(node, src, owner, inside_fn, defs, refs);
            true
        }
        _ => false,
    }
}

fn walk_py_stmt_refs(
    node: Node<'_>,
    src: &str,
    owner: Option<&str>,
    inside_fn: bool,
    defs: &mut Vec<Definition>,
    refs: &mut Vec<Reference>,
) -> bool {
    match node.kind() {
        "global_statement" | "nonlocal_statement" => {
            collect_identifier_children(node, refs);
            true
        }
        "delete_statement" => {
            recurse_py(node, src, owner, inside_fn, defs, refs);
            true
        }
        "raise_statement" => {
            collect_raise_from(node, refs);
            recurse_py(node, src, owner, inside_fn, defs, refs);
            true
        }
        "decorator" => {
            collect_decorator(node, src, owner, inside_fn, defs, refs);
            true
        }
        "attribute" => {
            if let Some(attr) = node.child_by_field_name("attribute") {
                refs.push(Reference {
                    start: attr.start_byte(),
                    end: attr.end_byte(),
                    kind: ReferenceKind::Method,
                });
            }
            recurse_py(node, src, owner, inside_fn, defs, refs);
            true
        }
        "identifier" => {
            if python_identifier_is_value(node) {
                refs.push(Reference {
                    start: node.start_byte(),
                    end: node.end_byte(),
                    kind: ReferenceKind::Call,
                });
            }
            true
        }
        _ => false,
    }
}

fn walk_py_references(
    node: Node<'_>,
    src: &str,
    owner: Option<&str>,
    inside_fn: bool,
    defs: &mut Vec<Definition>,
    refs: &mut Vec<Reference>,
) -> bool {
    walk_py_calls_and_imports(node, src, owner, inside_fn, defs, refs)
        || walk_py_stmt_refs(node, src, owner, inside_fn, defs, refs)
}

pub(super) fn walk_py(
    node: Node<'_>,
    src: &str,
    owner: Option<&str>,
    inside_fn: bool,
    defs: &mut Vec<Definition>,
    refs: &mut Vec<Reference>,
) {
    if walk_py_definitions(node, src, owner, inside_fn, defs, refs) {
        return;
    }
    if walk_py_references(node, src, owner, inside_fn, defs, refs) {
        return;
    }
    recurse_py(node, src, owner, inside_fn, defs, refs);
}
