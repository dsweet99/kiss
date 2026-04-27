//! Python AST extraction (Task 2).
//!
//! Uses the same tree-sitter grammar as `crate::parsing` to enumerate
//! function/method definitions and reference sites for `kiss mv`. Returned
//! offsets are byte offsets into the original source string.

use tree_sitter::{Node, Parser};

use crate::Language;

use super::ast_models::{
    AstResult, Definition, FallbackReason, ParseOutcome, Reference, ReferenceKind, SymbolKind,
};

pub(super) fn parse_python(content: &str) -> ParseOutcome {
    let mut parser = Parser::new();
    let lang = tree_sitter_python::LANGUAGE;
    if parser.set_language(&lang.into()).is_err() {
        return ParseOutcome::Fail(FallbackReason::ParserUnavailable);
    }
    let Some(tree) = parser.parse(content, None) else {
        return ParseOutcome::Fail(FallbackReason::ParseFailed);
    };
    let root = tree.root_node();
    if root.has_error() {
        return ParseOutcome::Fail(FallbackReason::ParseFailed);
    }
    let mut definitions = Vec::new();
    let mut references = Vec::new();
    walk_py(
        root,
        content,
        None,
        false,
        &mut definitions,
        &mut references,
    );
    ParseOutcome::Success(AstResult {
        definitions,
        references,
    })
}

pub(super) fn walk_py(
    node: Node<'_>,
    src: &str,
    owner: Option<&str>,
    inside_fn: bool,
    defs: &mut Vec<Definition>,
    refs: &mut Vec<Reference>,
) {
    match node.kind() {
        "decorated_definition" => {
            handle_decorated(node, src, owner, inside_fn, defs, refs);
        }
        "function_definition" | "async_function_definition" => {
            collect_py_def(node, node, src, owner, defs);
            recurse_py(node, src, None, true, defs, refs);
        }
        "class_definition" => {
            let name = name_text(node, src);
            if let Some(body) = node.child_by_field_name("body") {
                let mut c = body.walk();
                for child in body.children(&mut c) {
                    walk_py(child, src, name.as_deref(), false, defs, refs);
                }
            }
        }
        "call" => {
            collect_py_call(node, src, refs);
            recurse_py(node, src, owner, inside_fn, defs, refs);
        }
        "import_from_statement" | "import_statement" => {
            collect_py_import(node, src, refs);
        }
        "await" => {
            collect_identifier_children(node, refs);
            recurse_py(node, src, owner, inside_fn, defs, refs);
        }
        "global_statement" | "nonlocal_statement" | "delete_statement" => {
            collect_identifier_children(node, refs);
        }
        "raise_statement" => {
            collect_raise_from(node, refs);
            recurse_py(node, src, owner, inside_fn, defs, refs);
        }
        "decorator" => {
            collect_decorator(node, src, owner, inside_fn, defs, refs);
        }
        _ => recurse_py(node, src, owner, inside_fn, defs, refs),
    }
}

pub(super) fn collect_decorator(
    node: Node<'_>,
    src: &str,
    owner: Option<&str>,
    inside_fn: bool,
    defs: &mut Vec<Definition>,
    refs: &mut Vec<Reference>,
) {
    let mut c = node.walk();
    for child in node.children(&mut c) {
        match child.kind() {
            "identifier" => refs.push(Reference {
                start: child.start_byte(),
                end: child.end_byte(),
                kind: ReferenceKind::Call,
            }),
            "attribute" => {
                if let Some(attr) = child.child_by_field_name("attribute") {
                    refs.push(Reference {
                        start: attr.start_byte(),
                        end: attr.end_byte(),
                        kind: ReferenceKind::Method,
                    });
                }
            }
            "call" => {
                collect_py_call(child, src, refs);
                recurse_py(child, src, owner, inside_fn, defs, refs);
            }
            _ => {}
        }
    }
}

pub(super) fn collect_identifier_children(node: Node<'_>, refs: &mut Vec<Reference>) {
    let mut c = node.walk();
    for child in node.children(&mut c) {
        if child.kind() == "identifier" {
            refs.push(Reference {
                start: child.start_byte(),
                end: child.end_byte(),
                kind: ReferenceKind::Call,
            });
        }
    }
}

pub(super) fn collect_raise_from(node: Node<'_>, refs: &mut Vec<Reference>) {
    let cause = node.child_by_field_name("cause");
    if let Some(cause_node) = cause
        && cause_node.kind() == "identifier"
    {
        refs.push(Reference {
            start: cause_node.start_byte(),
            end: cause_node.end_byte(),
            kind: ReferenceKind::Call,
        });
    }
}

pub(super) fn handle_decorated(
    node: Node<'_>,
    src: &str,
    owner: Option<&str>,
    inside_fn: bool,
    defs: &mut Vec<Definition>,
    refs: &mut Vec<Reference>,
) {
    let mut c = node.walk();
    for child in node.children(&mut c) {
        if child.kind() == "decorator" {
            collect_decorator(child, src, owner, inside_fn, defs, refs);
        }
    }
    let Some(definition) = node.child_by_field_name("definition") else {
        recurse_py(node, src, owner, inside_fn, defs, refs);
        return;
    };
    match definition.kind() {
        "function_definition" | "async_function_definition" => {
            collect_py_def(definition, node, src, owner, defs);
            recurse_py(definition, src, None, true, defs, refs);
        }
        "class_definition" => {
            let name = name_text(definition, src);
            if let Some(body) = definition.child_by_field_name("body") {
                let mut c = body.walk();
                for child in body.children(&mut c) {
                    walk_py(child, src, name.as_deref(), false, defs, refs);
                }
            }
        }
        _ => recurse_py(definition, src, owner, inside_fn, defs, refs),
    }
}

pub(super) fn recurse_py(
    node: Node<'_>,
    src: &str,
    owner: Option<&str>,
    inside_fn: bool,
    defs: &mut Vec<Definition>,
    refs: &mut Vec<Reference>,
) {
    let mut c = node.walk();
    for child in node.children(&mut c) {
        walk_py(child, src, owner, inside_fn, defs, refs);
    }
}

pub(super) fn name_text(node: Node<'_>, src: &str) -> Option<String> {
    let n = node.child_by_field_name("name")?;
    let bytes = src.as_bytes();
    let start = n.start_byte();
    let end = n.end_byte();
    assert!(end <= bytes.len(), "name node end exceeds source");
    std::str::from_utf8(&bytes[start..end])
        .ok()
        .map(str::to_owned)
}

pub(super) fn collect_py_def(
    name_node: Node<'_>,
    span_node: Node<'_>,
    src: &str,
    owner: Option<&str>,
    defs: &mut Vec<Definition>,
) {
    let Some(name) = name_text(name_node, src) else {
        return;
    };
    let Some(ident) = name_node.child_by_field_name("name") else {
        return;
    };
    defs.push(Definition {
        name,
        owner: owner.map(str::to_owned),
        kind: if owner.is_some() {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        },
        start: span_node.start_byte(),
        end: span_node.end_byte(),
        name_start: ident.start_byte(),
        name_end: ident.end_byte(),
        language: Language::Python,
    });
}

pub(super) fn collect_py_call(node: Node<'_>, _src: &str, refs: &mut Vec<Reference>) {
    let Some(func) = node.child_by_field_name("function") else {
        return;
    };
    match func.kind() {
        "identifier" => refs.push(Reference {
            start: func.start_byte(),
            end: func.end_byte(),
            kind: ReferenceKind::Call,
        }),
        "attribute" => {
            if let Some(attr) = func.child_by_field_name("attribute") {
                refs.push(Reference {
                    start: attr.start_byte(),
                    end: attr.end_byte(),
                    kind: ReferenceKind::Method,
                });
            }
        }
        _ => {}
    }
}

pub(super) fn collect_py_import(node: Node<'_>, _src: &str, refs: &mut Vec<Reference>) {
    let mut c = node.walk();
    for child in node.children(&mut c) {
        match child.kind() {
            "dotted_name" | "identifier" => push_import_name(child, refs),
            "aliased_import" => {
                if let Some(name) = child.child_by_field_name("name") {
                    push_import_name(name, refs);
                }
            }
            _ => {}
        }
    }
}

pub(super) fn push_import_name(node: Node<'_>, refs: &mut Vec<Reference>) {
    let kind = node.kind();
    if kind == "identifier" {
        refs.push(Reference {
            start: node.start_byte(),
            end: node.end_byte(),
            kind: ReferenceKind::Import,
        });
        return;
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        if child.kind() == "identifier" {
            refs.push(Reference {
                start: child.start_byte(),
                end: child.end_byte(),
                kind: ReferenceKind::Import,
            });
        }
    }
}

#[cfg(test)]
#[path = "ast_python_test.rs"]
mod ast_python_test;
