use std::collections::HashSet;
use tree_sitter::Node;

fn patch_target_symbol(target: &str) -> Option<String> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(
        trimmed
            .rsplit('.')
            .next()
            .unwrap_or(trimmed)
            .to_string(),
    )
}

fn string_literal_value(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "string" => {
            let raw = &source[node.start_byte()..node.end_byte()];
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "string_content" {
                    return Some(source[child.start_byte()..child.end_byte()].to_string());
                }
            }
            Some(
                raw.trim_matches(|c| c == '"' || c == '\'')
                    .replace("\\\"", "\"")
                    .replace("\\'", "'"),
            )
        }
        _ => None,
    }
}

fn call_callee_name(node: Node, source: &str) -> Option<String> {
    match node.kind() {
        "identifier" => Some(source[node.start_byte()..node.end_byte()].to_string()),
        "attribute" => node
            .child_by_field_name("attribute")
            .map(|attr| source[attr.start_byte()..attr.end_byte()].to_string()),
        _ => None,
    }
}

fn is_patch_callee(name: &str) -> bool {
    name == "patch" || name.ends_with(".patch")
}

fn first_positional_string_arg(node: Node, source: &str) -> Option<String> {
    let args = node.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        match child.kind() {
            "string" => return string_literal_value(child, source),
            "keyword_argument" => {
                if let Some(val) = child.child_by_field_name("value")
                    && let Some(s) = string_literal_value(val, source)
                {
                    return Some(s);
                }
            }
            _ => {}
        }
    }
    None
}

fn patch_object_string_arg(node: Node, source: &str) -> Option<String> {
    let args = node.child_by_field_name("arguments")?;
    let mut cursor = args.walk();
    let mut positional = 0usize;
    for child in args.children(&mut cursor) {
        if child.kind() == "string" {
            positional += 1;
            if positional == 2 {
                return string_literal_value(child, source);
            }
        } else if child.kind() == "keyword_argument" {
            let name = child
                .child_by_field_name("name")
                .map(|n| source[n.start_byte()..n.end_byte()].to_string());
            if name.as_deref() == Some("new") || name.as_deref() == Some("new_callable") {
                continue;
            }
            if let Some(val) = child.child_by_field_name("value")
                && let Some(s) = string_literal_value(val, source)
            {
                return Some(s);
            }
        }
    }
    None
}

pub(super) fn record_patch_call(node: Node, source: &str, mocked: &mut HashSet<String>) {
    let Some(func) = node.child_by_field_name("function") else {
        return;
    };
    let Some(callee) = call_callee_name(func, source) else {
        return;
    };
    if !is_patch_callee(&callee) {
        return;
    }
    if callee == "patch.object" {
        if let Some(target) = patch_object_string_arg(node, source)
            && let Some(sym) = patch_target_symbol(&target)
        {
            mocked.insert(sym);
        }
        return;
    }
    if let Some(target) = first_positional_string_arg(node, source)
        && let Some(sym) = patch_target_symbol(&target)
    {
        mocked.insert(sym);
    }
}

#[cfg(test)]
mod collect_mock_patch_tests {
    use super::*;

    fn parse_call(src: &str) -> (tree_sitter::Tree, String) {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .unwrap();
        (parser.parse(src, None).unwrap(), src.to_string())
    }

    fn find_call_node<'a>(node: Node<'a>) -> Option<Node<'a>> {
        if node.kind() == "call" {
            return Some(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_call_node(child) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn patch_target_symbol_takes_last_segment() {
        assert_eq!(
            patch_target_symbol("pkg.mod.fn").as_deref(),
            Some("fn")
        );
        assert!(patch_target_symbol("  ").is_none());
    }

    #[test]
    fn is_patch_callee_recognizes_patch_variants() {
        assert!(is_patch_callee("patch"));
        assert!(is_patch_callee("unittest.mock.patch"));
        assert!(!is_patch_callee("patchy"));
    }

    #[test]
    fn record_patch_call_extracts_mock_target() {
        let (tree, src) = parse_call("patch('service.run_task', return_value=1)");
        let call = find_call_node(tree.root_node()).expect("call");
        let mut mocked = HashSet::new();
        record_patch_call(call, &src, &mut mocked);
        assert!(mocked.contains("run_task"));
    }

    #[test]
    fn patch_object_string_arg_extracts_second_positional() {
        let (tree, src) = parse_call("patch.object('mod.fn', 'target_fn')");
        let call = find_call_node(tree.root_node()).expect("call");
        assert_eq!(
            patch_object_string_arg(call, &src).as_deref(),
            Some("target_fn")
        );
    }

    #[test]
    fn first_positional_string_arg_reads_keyword_value() {
        let (tree, src) = parse_call("patch(new=1, target='pkg.symbol')");
        let call = find_call_node(tree.root_node()).expect("call");
        assert_eq!(
            first_positional_string_arg(call, &src).as_deref(),
            Some("pkg.symbol")
        );
    }

    #[test]
    fn patch_object_string_arg_skips_new_keyword() {
        let (tree, src) = parse_call("patch.object('mod.fn', new=1)");
        let call = find_call_node(tree.root_node()).expect("call");
        assert_eq!(patch_object_string_arg(call, &src), None);
    }

    #[test]
    fn record_patch_call_ignores_non_patch_callee() {
        let (tree, src) = parse_call("other('mod.fn')");
        let call = find_call_node(tree.root_node()).expect("call");
        let mut mocked = HashSet::new();
        record_patch_call(call, &src, &mut mocked);
        assert!(mocked.is_empty());
    }

    #[test]
    fn record_patch_call_keyword_target() {
        let (tree, src) = parse_call("patch(new=1, target='pkg.symbol')");
        let call = find_call_node(tree.root_node()).expect("call");
        let mut mocked = HashSet::new();
        record_patch_call(call, &src, &mut mocked);
        assert!(mocked.contains("symbol"));
    }

    #[test]
    fn string_literal_value_trims_quotes_without_content_node() {
        let (tree, src) = parse_call("patch(\"plain.module.func\")");
        let call = find_call_node(tree.root_node()).expect("call");
        let args = call.child_by_field_name("arguments").expect("args");
        let mut cursor = args.walk();
        let string_node = args
            .children(&mut cursor)
            .find(|c| c.kind() == "string")
            .expect("string arg");
        assert_eq!(
            string_literal_value(string_node, &src).as_deref(),
            Some("plain.module.func")
        );
    }
}
