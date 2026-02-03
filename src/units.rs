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
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                extract_from_node(child, source, units, false);
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
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                extract_from_node(child, source, units, true);
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                extract_from_node(child, source, units, inside_class);
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
    use crate::test_utils::parse_python_source;

    #[test]
    fn test_code_unit_kind_eq() {
        assert_eq!(CodeUnitKind::Function, CodeUnitKind::Function);
        assert_ne!(CodeUnitKind::Function, CodeUnitKind::Method);
    }

    #[test]
    fn test_extract_module() {
        let parsed = parse_python_source("x = 1");
        let units = extract_code_units(&parsed);
        assert!(units.iter().any(|u| u.kind == CodeUnitKind::Module));
    }

    #[test]
    fn test_extract_function() {
        let parsed = parse_python_source("def foo(): pass");
        let units = extract_code_units(&parsed);
        assert!(
            units
                .iter()
                .any(|u| u.kind == CodeUnitKind::Function && u.name == "foo")
        );
    }

    #[test]
    fn test_count_matches_extract_len() {
        let parsed = parse_python_source(
            "def outer():\n    def inner(): pass\nclass C:\n    def m(self): pass",
        );
        assert_eq!(count_code_units(&parsed), extract_code_units(&parsed).len());
    }

    #[test]
    fn test_extract_async_function() {
        let parsed = parse_python_source("async def bar(): pass");
        let units = extract_code_units(&parsed);
        assert!(
            units
                .iter()
                .any(|u| u.kind == CodeUnitKind::Function && u.name == "bar")
        );
    }

    #[test]
    fn test_extract_class() {
        let parsed = parse_python_source("class MyClass: pass");
        let units = extract_code_units(&parsed);
        assert!(
            units
                .iter()
                .any(|u| u.kind == CodeUnitKind::Class && u.name == "MyClass")
        );
    }

    #[test]
    fn test_extract_method() {
        let parsed = parse_python_source("class C:\n    def method(self): pass");
        let units = extract_code_units(&parsed);
        assert!(
            units
                .iter()
                .any(|u| u.kind == CodeUnitKind::Method && u.name == "method")
        );
    }

    #[test]
    fn test_nested_function_is_function() {
        let parsed = parse_python_source("def outer():\n    def inner(): pass");
        let units = extract_code_units(&parsed);
        let inner = units.iter().find(|u| u.name == "inner").unwrap();
        assert_eq!(inner.kind, CodeUnitKind::Function);
    }

    #[test]
    fn test_code_unit_positions() {
        let parsed = parse_python_source("def f(): pass");
        let units = extract_code_units(&parsed);
        let f = units.iter().find(|u| u.name == "f").unwrap();
        assert_eq!(f.start_line, 1);
        assert!(f.start_byte < f.end_byte);
    }

    #[test]
    fn test_get_child_by_field() {
        let parsed = parse_python_source("def foo(): pass");
        let root = parsed.tree.root_node();
        let func = root.child(0).unwrap();
        let name = get_child_by_field(func, "name", &parsed.source);
        assert_eq!(name, Some("foo".to_string()));
    }

    #[test]
    fn test_extract_from_node_recursion() {
        let parsed = parse_python_source("class A:\n    class B:\n        def m(self): pass");
        let units = extract_code_units(&parsed);
        assert!(units.iter().any(|u| u.name == "A"));
        assert!(units.iter().any(|u| u.name == "B"));
        assert!(
            units
                .iter()
                .any(|u| u.name == "m" && u.kind == CodeUnitKind::Method)
        );
    }

    #[test]
    fn test_code_unit_kind_all_variants() {
        let kinds = [
            CodeUnitKind::Function,
            CodeUnitKind::Method,
            CodeUnitKind::Class,
            CodeUnitKind::Module,
        ];
        assert_eq!(kinds.len(), 4);
    }

    #[test]
    fn test_code_unit_struct() {
        let unit = CodeUnit {
            kind: CodeUnitKind::Function,
            name: "foo".into(),
            start_line: 1,
            end_line: 5,
            start_byte: 0,
            end_byte: 50,
        };
        assert_eq!(unit.name, "foo");
        assert_eq!(unit.kind, CodeUnitKind::Function);
    }

    #[test]
    fn test_extract_from_node_direct() {
        let parsed = parse_python_source("def f(): pass\nclass C: pass");
        let mut units = Vec::new();
        extract_from_node(parsed.tree.root_node(), &parsed.source, &mut units, false);
        assert!(units.iter().any(|u| u.name == "f"));
        assert!(units.iter().any(|u| u.name == "C"));
    }

    #[test]
    fn test_code_unit_kind_as_str() {
        assert_eq!(CodeUnitKind::Function.as_str(), "function");
        assert_eq!(CodeUnitKind::Class.as_str(), "class");
        assert_eq!(CodeUnitKind::Method.as_str(), "method");
        assert_eq!(CodeUnitKind::Struct.as_str(), "struct");
        assert_eq!(CodeUnitKind::Enum.as_str(), "enum");
        assert_eq!(CodeUnitKind::TraitImplMethod.as_str(), "trait_impl_method");
    }
}
