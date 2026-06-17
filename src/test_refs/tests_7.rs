fn collect_type_refs_iterative(
    root: tree_sitter::Node,
    source: &str,
) -> std::collections::HashSet<String> {
    let mut refs = std::collections::HashSet::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "type" {
            super::collect::collect_type_refs(node, source, &mut refs);
        }
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
    }
    refs
}

#[test]
fn test_collect_type_refs_captures_attribute_leaf() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "def foo(x: pkg.MyType) -> other.Result:\n    pass\n";
    let tree = parser.parse(src, None).unwrap();

    let refs = collect_type_refs_iterative(tree.root_node(), src);

    assert!(refs.contains("MyType"));
    assert!(refs.contains("Result"));
}

#[test]
fn test_extract_import_from_binding_keeps_dotted_import_leaf() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "from pkg import sub.mod\n";
    let tree = parser.parse(src, None).unwrap();
    let import_node = tree.root_node().child(0).unwrap();
    let mut bindings = std::collections::HashMap::new();
    let mut alias_bindings = std::collections::HashMap::new();

    super::collect::extract_import_from_binding(
        import_node,
        src,
        &mut bindings,
        &mut alias_bindings,
    );

    let names = bindings.get("pkg").expect("pkg entry");
    assert!(
        names.contains("mod"),
        "dotted import leaf should be tracked"
    );
    assert!(alias_bindings.is_empty());
}

#[test]
fn test_collect_usage_refs_in_scope_captures_decorator_targets() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "@pytest.mark.parametrize('x', [make_case()])\ndef test_x(x):\n    assert x\n";
    let tree = parser.parse(src, None).unwrap();
    let mut refs = std::collections::HashSet::new();

    super::collect::collect_usage_refs_in_scope(tree.root_node(), src, &mut refs);

    assert!(refs.contains("pytest"));
    assert!(refs.contains("mark"));
    assert!(refs.contains("parametrize"));
    assert!(refs.contains("make_case"));
}
