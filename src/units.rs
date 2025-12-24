//! Code unit extraction from Python ASTs

use crate::parsing::ParsedFile;
use tree_sitter::Node;

/// The type of a code unit
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeUnitKind {
    Function,
    Method,
    Class,
    Module,
}

/// A code unit extracted from Python source
#[derive(Debug)]
pub struct CodeUnit {
    pub kind: CodeUnitKind,
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// Extracts all code units from a parsed file
pub fn extract_code_units(parsed: &ParsedFile) -> Vec<CodeUnit> {
    let mut units = Vec::new();
    let root = parsed.tree.root_node();

    // The module itself is a code unit
    units.push(CodeUnit {
        kind: CodeUnitKind::Module,
        name: parsed
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".to_string()),
        start_line: 1,
        end_line: root.end_position().row + 1,
        start_byte: 0,
        end_byte: parsed.source.len(),
    });

    // Walk the AST to find functions and classes
    extract_from_node(root, &parsed.source, &mut units, false);

    units
}

fn extract_from_node(node: Node, source: &str, units: &mut Vec<CodeUnit>, inside_class: bool) {
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            if let Some(name) = get_child_by_field(node, "name", source) {
                units.push(CodeUnit {
                    kind: if inside_class {
                        CodeUnitKind::Method
                    } else {
                        CodeUnitKind::Function
                    },
                    name,
                    start_line: node.start_position().row + 1,
                    end_line: node.end_position().row + 1,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                });
            }
            // Don't recurse into nested functions for now (they'd be Functions, not Methods)
            // But we do want to count them - recurse with inside_class=false
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    extract_from_node(child, source, units, false);
                }
            }
        }
        "class_definition" => {
            if let Some(name) = get_child_by_field(node, "name", source) {
                units.push(CodeUnit {
                    kind: CodeUnitKind::Class,
                    name,
                    start_line: node.start_position().row + 1,
                    end_line: node.end_position().row + 1,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                });
            }
            // Recurse into class body - methods are inside_class=true
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    extract_from_node(child, source, units, true);
                }
            }
        }
        _ => {
            // Recurse into children
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    extract_from_node(child, source, units, inside_class);
                }
            }
        }
    }
}

pub(crate) fn get_child_by_field(node: Node, field: &str, source: &str) -> Option<String> {
    node.child_by_field_name(field)
        .map(|n| source[n.start_byte()..n.end_byte()].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::{create_parser, parse_file};
    use std::io::Write;

    fn parse_source(code: &str) -> ParsedFile {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "{}", code).unwrap();
        let mut parser = create_parser().unwrap();
        parse_file(&mut parser, tmp.path()).unwrap()
    }

    #[test]
    fn test_code_unit_kind_eq() {
        assert_eq!(CodeUnitKind::Function, CodeUnitKind::Function);
        assert_ne!(CodeUnitKind::Function, CodeUnitKind::Method);
    }

    #[test]
    fn test_extract_module() {
        let parsed = parse_source("x = 1");
        let units = extract_code_units(&parsed);
        assert!(units.iter().any(|u| u.kind == CodeUnitKind::Module));
    }

    #[test]
    fn test_extract_function() {
        let parsed = parse_source("def foo(): pass");
        let units = extract_code_units(&parsed);
        assert!(units.iter().any(|u| u.kind == CodeUnitKind::Function && u.name == "foo"));
    }

    #[test]
    fn test_extract_async_function() {
        let parsed = parse_source("async def bar(): pass");
        let units = extract_code_units(&parsed);
        assert!(units.iter().any(|u| u.kind == CodeUnitKind::Function && u.name == "bar"));
    }

    #[test]
    fn test_extract_class() {
        let parsed = parse_source("class MyClass: pass");
        let units = extract_code_units(&parsed);
        assert!(units.iter().any(|u| u.kind == CodeUnitKind::Class && u.name == "MyClass"));
    }

    #[test]
    fn test_extract_method() {
        let parsed = parse_source("class C:\n    def method(self): pass");
        let units = extract_code_units(&parsed);
        assert!(units.iter().any(|u| u.kind == CodeUnitKind::Method && u.name == "method"));
    }

    #[test]
    fn test_nested_function_is_function() {
        let parsed = parse_source("def outer():\n    def inner(): pass");
        let units = extract_code_units(&parsed);
        let inner = units.iter().find(|u| u.name == "inner").unwrap();
        assert_eq!(inner.kind, CodeUnitKind::Function);
    }

    #[test]
    fn test_code_unit_positions() {
        let parsed = parse_source("def f(): pass");
        let units = extract_code_units(&parsed);
        let f = units.iter().find(|u| u.name == "f").unwrap();
        assert_eq!(f.start_line, 1);
        assert!(f.start_byte < f.end_byte);
    }

    #[test]
    fn test_get_child_by_field() {
        let parsed = parse_source("def foo(): pass");
        let root = parsed.tree.root_node();
        let func = root.child(0).unwrap();
        let name = get_child_by_field(func, "name", &parsed.source);
        assert_eq!(name, Some("foo".to_string()));
    }

    #[test]
    fn test_extract_from_node_recursion() {
        let parsed = parse_source("class A:\n    class B:\n        def m(self): pass");
        let units = extract_code_units(&parsed);
        assert!(units.iter().any(|u| u.name == "A"));
        assert!(units.iter().any(|u| u.name == "B"));
        assert!(units.iter().any(|u| u.name == "m" && u.kind == CodeUnitKind::Method));
    }

    #[test]
    fn test_code_unit_kind_all_variants() {
        let kinds = [CodeUnitKind::Function, CodeUnitKind::Method, CodeUnitKind::Class, CodeUnitKind::Module];
        assert_eq!(kinds.len(), 4);
    }

    #[test]
    fn test_code_unit_struct() {
        let unit = CodeUnit { kind: CodeUnitKind::Function, name: "foo".into(), start_line: 1, end_line: 5, start_byte: 0, end_byte: 50 };
        assert_eq!(unit.name, "foo");
        assert_eq!(unit.kind, CodeUnitKind::Function);
    }

    #[test]
    fn test_extract_from_node_direct() {
        let parsed = parse_source("def f(): pass\nclass C: pass");
        let mut units = Vec::new();
        extract_from_node(parsed.tree.root_node(), &parsed.source, &mut units, false);
        assert!(units.iter().any(|u| u.name == "f"));
        assert!(units.iter().any(|u| u.name == "C"));
    }
}

