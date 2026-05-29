use super::collect::{
    collect_call_target, collect_py_import_names_for_refs, collect_type_refs,
    extract_import_from_binding, extract_import_statement_modules, insert_identifier,
};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub(crate) fn collect_all_test_file_data(
    node: Node,
    source: &str,
    test_refs: &mut HashSet<String>,
    usage_refs: &mut HashSet<String>,
    import_bindings: &mut HashMap<String, HashSet<String>>,
) {
    process_test_file_ast_node(
        node,
        source,
        test_refs,
        usage_refs,
        import_bindings,
        true,
    );
}

pub(crate) fn collect_all_test_file_data_for_coverage_map(
    node: Node,
    source: &str,
    test_refs: &mut HashSet<String>,
    usage_refs: &mut HashSet<String>,
    import_bindings: &mut HashMap<String, HashSet<String>>,
) {
    process_test_file_ast_node(
        node,
        source,
        test_refs,
        usage_refs,
        import_bindings,
        false,
    );
}

pub(crate) fn process_test_file_ast_node(
    node: Node,
    source: &str,
    test_refs: &mut HashSet<String>,
    usage_refs: &mut HashSet<String>,
    import_bindings: &mut HashMap<String, HashSet<String>>,
    include_bare_identifiers: bool,
) {
    match node.kind() {
        "call" => {
            if let Some(func) = node.child_by_field_name("function") {
                collect_call_target(func, source, test_refs);
                collect_call_target(func, source, usage_refs);
            }
        }
        "import_from_statement" => {
            collect_py_import_names_for_refs(node, source, test_refs);
            extract_import_from_binding(node, source, import_bindings);
            return;
        }
        "import_statement" => {
            collect_py_import_names_for_refs(node, source, test_refs);
            extract_import_statement_modules(node, source, import_bindings);
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
        "identifier" if include_bare_identifiers => {
            insert_identifier(node, source, usage_refs);
        }
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        process_test_file_ast_node(
            child,
            source,
            test_refs,
            usage_refs,
            import_bindings,
            include_bare_identifiers,
        );
    }
}
