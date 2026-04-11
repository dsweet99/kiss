use crate::parsing::ParsedFile;
use tree_sitter::Node;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeUnitKind {
    Function,
    Method,
    Class,
    Module,
    Struct,
    Enum,
    TraitImplMethod,
}

impl CodeUnitKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Method => "method",
            Self::Class => "class",
            Self::Module => "module",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::TraitImplMethod => "trait_impl_method",
        }
    }
}

impl std::fmt::Display for CodeUnitKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug)]
pub struct CodeUnit {
    pub kind: CodeUnitKind,
    pub name: String,
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

pub fn extract_code_units(parsed: &ParsedFile) -> Vec<CodeUnit> {
    let mut units = Vec::new();
    let root = parsed.tree.root_node();

    units.push(CodeUnit {
        kind: CodeUnitKind::Module,
        name: parsed.path.file_stem().map_or_else(
            || "unknown".to_string(),
            |s| s.to_string_lossy().into_owned(),
        ),
        start_line: 1,
        end_line: root.end_position().row + 1,
        start_byte: 0,
        end_byte: parsed.source.len(),
    });

    extract_from_node(root, &parsed.source, &mut units, false);

    units
}

/// Fast-path for callers that only need the *count* of units.
///
/// This matches `extract_code_units(parsed).len()` but avoids allocations and string copies.
#[must_use]
pub fn count_code_units(parsed: &ParsedFile) -> usize {
    let root = parsed.tree.root_node();
    // Always include the synthetic module unit.
    1 + count_from_node(root)
}

fn count_from_node(node: Node) -> usize {
    match node.kind() {
        "function_definition" | "async_function_definition" | "class_definition" => {
            let mut count = usize::from(node.child_by_field_name("name").is_some());
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                count += count_from_node(child);
            }
            count
        }
        _ => {
            let mut count = 0;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                count += count_from_node(child);
            }
            count
        }
    }
}

fn extract_children(node: Node, source: &str, units: &mut Vec<CodeUnit>, inside_class: bool) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_from_node(child, source, units, inside_class);
    }
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
            extract_children(node, source, units, false);
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
            extract_children(node, source, units, true);
        }
        _ => extract_children(node, source, units, inside_class),
    }
}

pub(crate) fn get_child_by_field(node: Node, field: &str, source: &str) -> Option<String> {
    node.child_by_field_name(field)
        .map(|n| source[n.start_byte()..n.end_byte()].to_string())
}

#[cfg(test)]
#[path = "units_test.rs"]
mod tests;
