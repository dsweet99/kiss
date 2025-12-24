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

