//! Test References - detect code units that may lack test coverage

use crate::parsing::ParsedFile;
use crate::units::get_child_by_field;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tree_sitter::Node;

/// A code unit definition (function, method, or class)
#[derive(Debug, Clone)]
pub struct CodeDefinition {
    pub name: String,
    pub kind: &'static str, // "function", "method", "class"
    pub file: PathBuf,
    pub line: usize,
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

/// Check if a file path is a test file (test_*.py, *_test.py, or in tests/ directory)
pub fn is_test_file(path: &std::path::Path) -> bool {
    // Check for tests/ or test/ directory in path
    if path.components().any(|c| {
        let s = c.as_os_str();
        s == "tests" || s == "test"
    }) {
        return true;
    }
    
    // Check filename patterns
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        name.starts_with("test_") || name.ends_with("_test.py")
    } else {
        false
    }
}

/// Check if a parsed file contains pytest or unittest imports (fallback heuristic)
pub fn has_test_framework_import(node: Node, source: &str) -> bool {
    let mut cursor = node.walk();
    
    for child in node.children(&mut cursor) {
        match child.kind() {
            "import_statement" => {
                // import pytest, import unittest
                if contains_test_module_name(child, source) {
                    return true;
                }
            }
            "import_from_statement" => {
                // from pytest import ..., from unittest import ...
                if let Some(module) = child.child_by_field_name("module_name") {
                    let module_name = &source[module.start_byte()..module.end_byte()];
                    if module_name == "pytest" || module_name.starts_with("pytest.")
                        || module_name == "unittest" || module_name.starts_with("unittest.")
                    {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    
    false
}

/// Check if an import statement contains pytest or unittest
fn contains_test_module_name(node: Node, source: &str) -> bool {
    let mut cursor = node.walk();
    
    for child in node.children(&mut cursor) {
        match child.kind() {
            "dotted_name" => {
                let name = &source[child.start_byte()..child.end_byte()];
                if name == "pytest" || name == "unittest" {
                    return true;
                }
            }
            "aliased_import" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    let name = &source[name_node.start_byte()..name_node.end_byte()];
                    if name == "pytest" || name == "unittest" {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    
    false
}

/// Analyze test references across all parsed files
pub fn analyze_test_refs(parsed_files: &[&ParsedFile]) -> TestRefAnalysis {
    let mut definitions = Vec::new();
    let mut test_references = HashSet::new();

    for parsed in parsed_files {
        // A file is a test file if:
        // 1. It matches naming patterns (test_*.py, *_test.py) or is in tests/ directory
        // 2. Or it imports pytest/unittest (fallback heuristic)
        let is_test = is_test_file(&parsed.path) 
            || has_test_framework_import(parsed.tree.root_node(), &parsed.source);
        
        if is_test {
            // Collect references from test files
            collect_references(parsed.tree.root_node(), &parsed.source, &mut test_references);
        } else {
            // Collect definitions from source files
            collect_definitions(
                parsed.tree.root_node(),
                &parsed.source,
                &parsed.path,
                &mut definitions,
                false,
            );
        }
    }

    // Find unreferenced definitions
    let unreferenced = definitions
        .iter()
        .filter(|def| !test_references.contains(&def.name))
        .cloned()
        .collect();

    TestRefAnalysis {
        definitions,
        test_references,
        unreferenced,
    }
}

fn try_add_def(node: Node, source: &str, file: &Path, defs: &mut Vec<CodeDefinition>, kind: &'static str) {
    if let Some(name) = get_child_by_field(node, "name", source)
        && (!name.starts_with('_') || name == "__init__") {
            defs.push(CodeDefinition { name, kind, file: file.to_path_buf(), line: node.start_position().row + 1 });
        }
}

fn recurse_children(node: Node, source: &str, file: &Path, defs: &mut Vec<CodeDefinition>, inside_class: bool) {
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) { collect_definitions(child, source, file, defs, inside_class); }
    }
}

fn collect_definitions(node: Node, source: &str, file: &Path, defs: &mut Vec<CodeDefinition>, inside_class: bool) {
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            try_add_def(node, source, file, defs, if inside_class { "method" } else { "function" });
            recurse_children(node, source, file, defs, false);
        }
        "class_definition" => {
            try_add_def(node, source, file, defs, "class");
            recurse_children(node, source, file, defs, true);
        }
        _ => recurse_children(node, source, file, defs, inside_class),
    }
}

/// Collect all name references from a node (imports, calls, attribute access)
fn collect_references(node: Node, source: &str, refs: &mut HashSet<String>) {
    match node.kind() {
        // Function/method calls
        "call" => {
            if let Some(func) = node.child_by_field_name("function") {
                collect_call_target(func, source, refs);
            }
        }
        // Import statements
        "import_statement" | "import_from_statement" => {
            collect_import_names(node, source, refs);
        }
        // Simple identifier references (variable access)
        "identifier" => {
            let name = source[node.start_byte()..node.end_byte()].to_string();
            refs.insert(name);
        }
        _ => {}
    }

    // Recurse to children
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            collect_references(child, source, refs);
        }
    }
}

/// Extract the target name from a call expression
fn collect_call_target(node: Node, source: &str, refs: &mut HashSet<String>) {
    match node.kind() {
        "identifier" => {
            let name = source[node.start_byte()..node.end_byte()].to_string();
            refs.insert(name);
        }
        "attribute" => {
            // For method calls like obj.method(), extract "method"
            if let Some(attr) = node.child_by_field_name("attribute") {
                let name = source[attr.start_byte()..attr.end_byte()].to_string();
                refs.insert(name);
            }
            // Also extract the object for chained calls
            if let Some(obj) = node.child_by_field_name("object") {
                collect_call_target(obj, source, refs);
            }
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
                // Get the first identifier (the module/name being imported)
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    if inner.kind() == "identifier" {
                        let name = source[inner.start_byte()..inner.end_byte()].to_string();
                        refs.insert(name);
                    }
                }
            }
            "identifier" => {
                let name = source[child.start_byte()..child.end_byte()].to_string();
                refs.insert(name);
            }
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
    }

    #[test]
    fn test_is_test_file_by_path_component() {
        // Files in tests/ directory should be detected
        assert!(is_test_file(Path::new("tests/conftest.py")));
        assert!(is_test_file(Path::new("tests/helpers.py")));
        assert!(is_test_file(Path::new("/project/tests/unit/test_utils.py")));
        
        // Files in test/ directory should also be detected
        assert!(is_test_file(Path::new("test/integration.py")));
        
        // Regular source files should not be detected
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

