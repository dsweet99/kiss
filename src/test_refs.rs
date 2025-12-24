//! Test References - detect code units that may lack test coverage

use crate::parsing::ParsedFile;
use crate::units::get_child_by_field;
use std::collections::HashSet;
use std::path::PathBuf;
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

/// Check if a file path is a test file (test_*.py or *_test.py)
pub fn is_test_file(path: &std::path::Path) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        name.starts_with("test_") || name.ends_with("_test.py")
    } else {
        false
    }
}

/// Analyze test references across all parsed files
pub fn analyze_test_refs(parsed_files: &[&ParsedFile]) -> TestRefAnalysis {
    let mut definitions = Vec::new();
    let mut test_references = HashSet::new();

    for parsed in parsed_files {
        if is_test_file(&parsed.path) {
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

/// Collect all function, method, and class definitions from a node
fn collect_definitions(
    node: Node,
    source: &str,
    file: &PathBuf,
    defs: &mut Vec<CodeDefinition>,
    inside_class: bool,
) {
    match node.kind() {
        "function_definition" | "async_function_definition" => {
            if let Some(name) = get_child_by_field(node, "name", source) {
                // Skip private/dunder methods unless they're significant
                if !name.starts_with('_') || name == "__init__" {
                    defs.push(CodeDefinition {
                        name,
                        kind: if inside_class { "method" } else { "function" },
                        file: file.clone(),
                        line: node.start_position().row + 1,
                    });
                }
            }
            // Recurse into function body for nested definitions
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    collect_definitions(child, source, file, defs, false);
                }
            }
        }
        "class_definition" => {
            if let Some(name) = get_child_by_field(node, "name", source) {
                defs.push(CodeDefinition {
                    name,
                    kind: "class",
                    file: file.clone(),
                    line: node.start_position().row + 1,
                });
            }
            // Recurse into class body for methods
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    collect_definitions(child, source, file, defs, true);
                }
            }
        }
        _ => {
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    collect_definitions(child, source, file, defs, inside_class);
                }
            }
        }
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
    fn test_is_test_file() {
        assert!(is_test_file(Path::new("test_foo.py")));
        assert!(is_test_file(Path::new("foo_test.py")));
        assert!(is_test_file(Path::new("/some/path/test_bar.py")));
        assert!(!is_test_file(Path::new("foo.py")));
        assert!(!is_test_file(Path::new("testing.py")));
        assert!(!is_test_file(Path::new("my_test_helper.py")));
    }
}

