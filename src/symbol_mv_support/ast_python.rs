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
        trait_impls: Vec::new(),
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
            collect_py_def(node, node, src, owner, defs);
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
        // Bare attribute access `obj.attr` not heading a call, e.g.
        // `@property` reads (`b.area`), method-as-value reads
        // (`cb = obj.handler`), and attribute writes (`obj.field = …`).
        // Without this arm the trailing attribute identifier is
        // suppressed by `python_identifier_is_value_reference`'s
        // `"attribute" if same("attribute") => false` guard, so every
        // such site silently escapes the rename plan (KPOP round 9 H1).
        // The planner dedupes by (start, end) so overlap with the
        // call/decorator arms that already emit the same span is
        // harmless. Receiver/owner filtering happens later in
        // `reference_admits`, preserving R3 disambiguation.
        "attribute" => {
            if let Some(attr) = node.child_by_field_name("attribute") {
                refs.push(Reference {
                    start: attr.start_byte(),
                    end: attr.end_byte(),
                    kind: ReferenceKind::Method,
                });
            }
            recurse_py(node, src, owner, inside_fn, defs, refs);
        }
        "identifier" => {
            if python_identifier_is_value_reference(node) {
                refs.push(Reference {
                    start: node.start_byte(),
                    end: node.end_byte(),
                    kind: ReferenceKind::Call,
                });
            }
        }
        _ => recurse_py(node, src, owner, inside_fn, defs, refs),
    }
}

/// Decide whether a bare `identifier` node represents a *use* of a name
/// (i.e. a reference site) rather than a binding/definition site. Catches
/// callback-style uses like `map(my_fn, …)`, kwarg values like `key=my_fn`,
/// assignment RHS like `ref = my_fn`, container literals, return values,
/// etc. — fixes KPOP round 6 H2 ("function-as-value not renamed").
///
/// References that are emitted via dedicated branches above (calls,
/// decorators, imports, await, raise-from, global/nonlocal/delete) are
/// also re-emitted here when the walker recurses into them; the planner
/// dedupes by (start, end), so duplicates are harmless.
fn python_identifier_is_value_reference(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return true;
    };
    let same = |field: &str| {
        parent
            .child_by_field_name(field)
            .is_some_and(|n| n.id() == node.id())
    };
    match parent.kind() {
        // Definition NAME positions are bindings, not references.
        "function_definition" | "async_function_definition" | "class_definition"
        | "typed_parameter" | "default_parameter" | "typed_default_parameter" => false,
        // Bare parameter names: `def f(x):` parses `x` as an identifier
        // child of a `parameters` node.
        "parameters"
        | "lambda_parameters"
        | "import_from_statement"
        | "import_statement"
        | "aliased_import"
        | "dotted_name"
        | "decorator"
        | "global_statement"
        | "nonlocal_statement"
        | "delete_statement"
        | "keyword_argument"
            if same("name") => false,
        // Attribute access `obj.attr`: the `attr` part is the attribute
        // (handled separately as Method when it heads a call); the
        // `object` part IS a name reference and falls through.
        "attribute" if same("attribute") => false,
        _ => true,
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
            collect_py_def(definition, definition, src, owner, defs);
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
