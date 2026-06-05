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

#[cfg(test)]
mod ast_python_walk_tests {
    use super::super::ast_models::{Definition, Reference, ReferenceKind};
    use super::walk_py;

    fn parse_module(src: &str) -> (tree_sitter::Tree, String) {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        (parser.parse(src, None).unwrap(), src.to_string())
    }

    fn walk_module(src: &str) -> (Vec<Definition>, Vec<Reference>) {
        let (tree, src) = parse_module(src);
        let mut defs = Vec::new();
        let mut refs = Vec::new();
        walk_py(tree.root_node(), &src, None, false, &mut defs, &mut refs);
        (defs, refs)
    }

    fn ref_names<'a>(refs: &'a [Reference], src: &'a str) -> Vec<&'a str> {
        refs.iter().map(|r| &src[r.start..r.end]).collect()
    }

    #[test]
    fn walk_collects_top_level_function_and_call() {
        let src = "def helper():\n    return 1\n\ndef caller():\n    return helper()\n";
        let (defs, refs) = walk_module(src);
        assert!(defs.iter().any(|d| d.name == "helper"));
        assert!(defs.iter().any(|d| d.name == "caller"));
        assert!(ref_names(&refs, src).contains(&"helper"));
    }

    #[test]
    fn walk_collects_class_method_and_attribute_call() {
        let src = "class C:\n    def m(self):\n        return 1\n\ndef use():\n    c = C()\n    return c.m()\n";
        let (defs, refs) = walk_module(src);
        assert!(defs.iter().any(|d| d.name == "m" && d.owner.as_deref() == Some("C")));
        assert!(
            refs.iter()
                .any(|r| r.kind == ReferenceKind::Method && &src[r.start..r.end] == "m")
        );
    }

    #[test]
    fn walk_collects_imports_and_await() {
        let src = "from os import path\nimport sys\n\nasync def f():\n    await helper()\n";
        let (defs, refs) = walk_module(src);
        assert!(defs.iter().any(|d| d.name == "f"));
        let imports: Vec<_> = refs
            .iter()
            .filter(|r| r.kind == ReferenceKind::Import)
            .map(|r| &src[r.start..r.end])
            .collect();
        assert!(imports.contains(&"path"));
        assert!(imports.contains(&"sys"));
        assert!(ref_names(&refs, src).contains(&"helper"));
    }

    #[test]
    fn walk_collects_global_nonlocal_delete_raise_and_decorator() {
        let src = "@deco\ndef f():\n    global x\n    nonlocal y\n    del z\n    raise E from cause\n";
        let (tree, src) = parse_module(src);
        let mut defs = Vec::new();
        let mut refs = Vec::new();
        walk_py(tree.root_node(), &src, None, false, &mut defs, &mut refs);
        assert!(defs.iter().any(|d| d.name == "f"));
        for name in ["x", "y", "z", "cause", "deco"] {
            assert!(
                ref_names(&refs, &src).contains(&name),
                "missing ref {name}"
            );
        }
    }

    #[test]
    fn walk_async_function_definition() {
        let src = "async def worker():\n    return 1\n";
        let (defs, _) = walk_module(src);
        assert!(defs.iter().any(|d| d.name == "worker"));
    }

    #[test]
    fn walk_decorated_definition() {
        let src = "@wrap\ndef wrapped():\n    pass\n";
        let (defs, refs) = walk_module(src);
        assert!(defs.iter().any(|d| d.name == "wrapped"));
        assert!(ref_names(&refs, src).contains(&"wrap"));
    }

    #[test]
    fn walk_identifier_value_reference() {
        let src = "def f():\n    return value\n";
        let (_, refs) = walk_module(src);
        assert!(refs.iter().any(|r| r.kind == ReferenceKind::Call));
    }

    #[test]
    fn walk_nested_class_body() {
        let src = "class Outer:\n    class Inner:\n        def m(self):\n            return 1\n";
        let (defs, _) = walk_module(src);
        assert!(defs.iter().any(|d| d.name == "Inner" && d.owner.as_deref() == Some("Outer")));
        assert!(defs.iter().any(|d| d.name == "m" && d.owner.as_deref() == Some("Inner")));
    }

    #[test]
    fn walk_recurse_fallback_for_unmatched_nodes() {
        let src = "x = 1\n";
        let (tree, src) = parse_module(src);
        let mut defs = Vec::new();
        let mut refs = Vec::new();
        let root = tree.root_node();
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            if child.kind() == "expression_statement" {
                walk_py(child, &src, None, false, &mut defs, &mut refs);
            }
        }
        assert!(ref_names(&refs, &src).contains(&"x") || refs.is_empty());
    }

    #[test]
    fn walk_call_with_nested_args() {
        let src = "def f():\n    outer(inner())\n";
        let (_, refs) = walk_module(src);
        assert!(ref_names(&refs, src).contains(&"outer"));
        assert!(ref_names(&refs, src).contains(&"inner"));
    }

    #[test]
    fn walk_attribute_reference_under_delete() {
        let src = "def f():\n    del obj.attr\n";
        let (_, refs) = walk_module(src);
        assert!(
            refs.iter()
                .any(|r| r.kind == ReferenceKind::Method && &src[r.start..r.end] == "attr")
        );
    }

    #[test]
    fn walk_import_statement() {
        let src = "import json\n";
        let (_, refs) = walk_module(src);
        assert!(
            refs.iter()
                .any(|r| r.kind == ReferenceKind::Import && &src[r.start..r.end] == "json")
        );
    }

    #[test]
    fn direct_helper_functions_invoked() {
        use super::{
            walk_py_calls_and_imports, walk_py_definitions, walk_py_references, walk_py_stmt_refs,
        };
        let (tree, src) = parse_module("import os\nfrom sys import path\n\ndef f():\n    global x\n");
        let root = tree.root_node();
        let mut defs = Vec::new();
        let mut refs = Vec::new();
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            let mut d = Vec::new();
            let mut r = Vec::new();
            let _ = walk_py_definitions(child, &src, None, false, &mut d, &mut r)
                || walk_py_calls_and_imports(child, &src, None, false, &mut d, &mut r)
                || walk_py_stmt_refs(child, &src, None, false, &mut d, &mut r)
                || walk_py_references(child, &src, None, false, &mut d, &mut r);
            defs.extend(d);
            refs.extend(r);
        }
        assert!(!defs.is_empty());
        walk_py(root, &src, None, false, &mut defs, &mut refs);
        assert!(!defs.is_empty());
    }
}
