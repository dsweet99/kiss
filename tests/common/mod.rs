#![allow(dead_code)]

use kiss::parsing::{ParsedFile, create_parser, parse_file};
use std::io::Write;
use tree_sitter::Node;

pub fn parse_python_source(code: &str) -> ParsedFile {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp, "{code}").unwrap();
    let mut parser = create_parser().expect("parser should initialize");
    parse_file(&mut parser, tmp.path()).expect("should parse temp source")
}

pub fn first_function_node(p: &ParsedFile) -> Node<'_> {
    let root = p.tree.root_node();
    for i in 0..root.child_count() {
        if let Some(node) = root.child(i)
            && node.kind() == "function_definition"
        {
            return node;
        }
    }

    for i in 0..root.child_count() {
        if let Some(node) = root.child(i)
            && node.kind() == "decorated_definition"
        {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "function_definition" {
                    return child;
                }
            }
        }
    }

    panic!("function_definition");
}

pub fn first_function_or_async_node(p: &ParsedFile) -> Node<'_> {
    let root = p.tree.root_node();
    (0..root.child_count())
        .filter_map(|i| root.child(i))
        .find(|n| n.kind() == "function_definition" || n.kind() == "async_function_definition")
        .expect("function_definition")
}
