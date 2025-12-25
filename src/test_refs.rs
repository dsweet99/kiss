//! Test References - detect code units that may lack test coverage

use crate::parsing::ParsedFile;
use crate::units::{get_child_by_field, CodeUnitKind};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tree_sitter::Node;

/// A code unit definition (function, method, or class)
#[derive(Debug, Clone)]
pub struct CodeDefinition {
    pub name: String,
    pub kind: CodeUnitKind,
    pub file: PathBuf,
    pub line: usize,
    /// For methods, the name of the containing class
    pub containing_class: Option<String>,
}

/// Result of test reference analysis
#[derive(Debug)]
pub struct TestRefAnalysis {
    /// All definitions found in source files
    pub definitions: Vec<CodeDefinition>,
    /// Names referenced in test files
    pub test_references: HashSet<String>,
    /// Definitions not referenced by any test
    pub unreferenced: Vec<CodeDefinition>,
}

fn has_python_test_naming(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| (name.starts_with("test_") && name.ends_with(".py")) || name.ends_with("_test.py"))
}

/// Check if a file path is a test file (test_*.py or *_test.py)
/// Note: Being in a tests/ directory alone is NOT sufficient - file must have test naming
#[must_use]
pub fn is_test_file(path: &std::path::Path) -> bool {
    has_python_test_naming(path)
}

fn is_test_framework(name: &str) -> bool {
    name == "pytest" || name == "unittest" || name.starts_with("pytest.") || name.starts_with("unittest.")
}

fn is_test_framework_import_from(child: Node, source: &str) -> bool {
    child.child_by_field_name("module_name")
        .map(|m| &source[m.start_byte()..m.end_byte()])
        .is_some_and(is_test_framework)
}

fn contains_test_module_name(node: Node, source: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let name = match child.kind() {
            "dotted_name" => Some(&source[child.start_byte()..child.end_byte()]),
            "aliased_import" => child.child_by_field_name("name").map(|n| &source[n.start_byte()..n.end_byte()]),
            _ => None,
        };
        if name.is_some_and(|n| n == "pytest" || n == "unittest") { return true; }
    }
    false
}

/// Check if a parsed file contains pytest or unittest imports (fallback heuristic)
pub fn has_test_framework_import(node: Node, source: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_statement" if contains_test_module_name(child, source) => return true,
            "import_from_statement" if is_test_framework_import_from(child, source) => return true,
            _ => {}
        }
    }
    false
}

fn is_python_test_file(parsed: &ParsedFile) -> bool {
    is_test_file(&parsed.path) || has_test_framework_import(parsed.tree.root_node(), &parsed.source)
}

/// Analyze test references across all parsed files
pub fn analyze_test_refs(parsed_files: &[&ParsedFile]) -> TestRefAnalysis {
    let mut definitions = Vec::new();
    let mut test_references = HashSet::new();

    for parsed in parsed_files {
        if is_python_test_file(parsed) {
            collect_references(parsed.tree.root_node(), &parsed.source, &mut test_references);
        } else {
            collect_definitions(parsed.tree.root_node(), &parsed.source, &parsed.path, &mut definitions, false, None);
        }
    }

    // Mark __init__ as covered if its class is referenced (instantiation calls __init__ implicitly)
    let class_names: HashSet<_> = definitions.iter()
        .filter(|d| d.kind == CodeUnitKind::Class && test_references.contains(&d.name))
        .map(|d| d.name.clone())
        .collect();
    
    let unreferenced = definitions.iter()
        .filter(|def| {
            if test_references.contains(&def.name) {
                return false;
            }
            // __init__ is covered if its containing class is referenced
            if def.name == "__init__"
                && let Some(ref class_name) = def.containing_class {
                    return !class_names.contains(class_name);
                }
            true
        })
        .cloned()
        .collect();
    
    TestRefAnalysis { definitions, test_references, unreferenced }
}

fn try_add_def(node: Node, source: &str, file: &Path, defs: &mut Vec<CodeDefinition>, kind: CodeUnitKind, containing_class: Option<String>) {
    if let Some(name) = get_child_by_field(node, "name", source)
        && (!name.starts_with('_') || name == "__init__") {
            defs.push(CodeDefinition { name, kind, file: file.to_path_buf(), line: node.start_position().row + 1, containing_class });
        }
}

fn collect_definitions(node: Node, source: &str, file: &Path, defs: &mut Vec<CodeDefinition>, inside_class: bool, class_name: Option<&str>) {
    let (next_inside, next_class_name) = match node.kind() {
        "function_definition" | "async_function_definition" => {
            let kind = if inside_class { CodeUnitKind::Method } else { CodeUnitKind::Function };
            try_add_def(node, source, file, defs, kind, class_name.map(String::from));
            (false, None)
        }
        "class_definition" => {
            try_add_def(node, source, file, defs, CodeUnitKind::Class, None);
            let name = get_child_by_field(node, "name", source);
            (true, name)
        }
        _ => (inside_class, class_name.map(String::from).as_deref().map(std::string::ToString::to_string)),
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_definitions(child, source, file, defs, next_inside, next_class_name.as_deref());
    }
}

fn insert_identifier(node: Node, source: &str, refs: &mut HashSet<String>) {
    refs.insert(source[node.start_byte()..node.end_byte()].to_string());
}

/// Collect all name references from a node (imports, calls, attribute access)
fn collect_references(node: Node, source: &str, refs: &mut HashSet<String>) {
    match node.kind() {
        "call" => if let Some(func) = node.child_by_field_name("function") {
            collect_call_target(func, source, refs);
        },
        "import_statement" | "import_from_statement" => collect_import_names(node, source, refs),
        "identifier" => insert_identifier(node, source, refs),
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_references(child, source, refs);
    }
}

/// Extract the target name from a call expression
fn collect_call_target(node: Node, source: &str, refs: &mut HashSet<String>) {
    match node.kind() {
        "identifier" => insert_identifier(node, source, refs),
        "attribute" => {
            if let Some(attr) = node.child_by_field_name("attribute") { insert_identifier(attr, source, refs); }
            if let Some(obj) = node.child_by_field_name("object") { collect_call_target(obj, source, refs); }
        }
        _ => {}
    }
}

/// Extract imported names from import statements
fn collect_import_names(node: Node, source: &str, refs: &mut HashSet<String>) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "dotted_name" | "aliased_import" => {
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    if inner.kind() == "identifier" { insert_identifier(inner, source, refs); }
                }
            }
            "identifier" => insert_identifier(child, source, refs),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_is_test_file_by_name() {
        assert!(is_test_file(Path::new("test_foo.py")));
        assert!(is_test_file(Path::new("foo_test.py")));
        assert!(is_test_file(Path::new("/some/path/test_bar.py")));
        assert!(!is_test_file(Path::new("foo.py")));
        assert!(!is_test_file(Path::new("testing.py")));
        assert!(!is_test_file(Path::new("my_test_helper.py")));
        assert!(!is_test_file(Path::new("test_foo.txt")), "non-.py should not match");
        assert!(!is_test_file(Path::new("test_data.json")), "non-.py should not match");
    }

    #[test]
    fn test_is_test_file_requires_naming_pattern() {
        assert!(is_test_file(Path::new("test_utils.py")));
        assert!(is_test_file(Path::new("utils_test.py")));
        assert!(is_test_file(Path::new("/project/tests/unit/test_utils.py")));
        assert!(!is_test_file(Path::new("tests/conftest.py")));
        assert!(!is_test_file(Path::new("tests/helpers.py")));
        assert!(!is_test_file(Path::new("src/utils.py")));
        assert!(!is_test_file(Path::new("myproject/testing_utils.py")));
    }

    #[test]
    fn test_has_test_framework_import() {
        use crate::parsing::create_parser;
        let mut parser = create_parser().unwrap();
        
        let mut check = |src: &str| {
            let tree = parser.parse(src, None).unwrap();
            has_test_framework_import(tree.root_node(), src)
        };
        
        assert!(check("import pytest\n\ndef test_foo():\n    pass\n"));
        assert!(check("import unittest\n\nclass TestCase(unittest.TestCase):\n    pass\n"));
        assert!(check("from pytest import fixture\n\n@fixture\ndef my_fixture():\n    pass\n"));
        assert!(check("import pytest as pt\n"));
        assert!(!check("import os\nimport sys\n\ndef main():\n    pass\n"));
    }
}

