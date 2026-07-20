#![allow(clippy::let_unit_value)]

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
    assert!(
        !is_test_file(Path::new("test_foo.txt")),
        "non-.py should not match"
    );
    assert!(
        !is_test_file(Path::new("test_data.json")),
        "non-.py should not match"
    );
}

#[test]
fn test_is_test_file_requires_naming_pattern() {
    assert!(is_test_file(Path::new("test_utils.py")));
    assert!(is_test_file(Path::new("utils_test.py")));
    assert!(is_test_file(Path::new("/project/tests/unit/test_utils.py")));
    assert!(
        is_test_file(Path::new("tests/conftest.py")),
        "conftest.py is pytest infrastructure"
    );
    assert!(
        is_test_file(Path::new("conftest.py")),
        "conftest.py at any level"
    );
    assert!(
        is_test_file(Path::new("conftest.PY")),
        "pytest conftest basename is case-insensitive on disk"
    );
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
    assert!(check(
        "import unittest\n\nclass TestCase(unittest.TestCase):\n    pass\n"
    ));
    assert!(check(
        "from pytest import fixture\n\n@fixture\ndef my_fixture():\n    pass\n"
    ));
    assert!(check("import pytest as pt\n"));
    assert!(!check("import os\nimport sys\n\ndef main():\n    pass\n"));
}

#[test]
fn test_is_in_test_directory() {
    assert!(is_in_test_directory(Path::new("tests/helpers.py")));
    assert!(is_in_test_directory(Path::new("tests/unit/helpers.py")));
    assert!(is_in_test_directory(Path::new("test/helpers.py")));
    assert!(is_in_test_directory(Path::new(
        "/project/tests/conftest.py"
    )));
    assert!(!is_in_test_directory(Path::new("src/utils.py")));
    assert!(!is_in_test_directory(Path::new("testing/utils.py")));
}

#[test]
fn test_collect_definitions_skips_test_functions() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "def helper():\n    pass\n\ndef test_helper():\n    pass\n";
    let tree = parser.parse(src, None).unwrap();
    let mut defs = Vec::new();
    collect_definitions(
        tree.root_node(),
        src,
        Path::new("utils.py"),
        &mut defs,
        false,
        None,
    );
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, vec!["helper"]);
}

#[test]
fn test_nested_functions_not_tracked_for_coverage() {
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();

    let src =
        "def outer():\n    def nested_helper():\n        return 42\n    return nested_helper()\n";
    let tree = parser.parse(src, None).unwrap();
    let file = ParsedFile {
        path: PathBuf::from("mymodule.py"),
        source: src.to_string(),
        tree,
    };

    let src_test = "from mymodule import outer\ndef test_outer():\n    outer()\n";
    let tree_test = parser.parse(src_test, None).unwrap();
    let file_test = ParsedFile {
        path: PathBuf::from("test_mymodule.py"),
        source: src_test.to_string(),
        tree: tree_test,
    };

    let analysis = analyze_test_refs(&[&file, &file_test], None);

    let def_names: Vec<&str> = analysis
        .definitions
        .iter()
        .map(|d| d.name.as_str())
        .collect();
    assert!(
        !def_names.contains(&"nested_helper"),
        "Nested function should not be tracked for coverage, but found: {def_names:?}"
    );
}

#[test]
fn test_same_name_different_files_disambiguated_by_module() {
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();

    let src_a = "def helper():\n    pass\n";
    let tree_a = parser.parse(src_a, None).unwrap();
    let file_a = ParsedFile {
        path: PathBuf::from("alpha.py"),
        source: src_a.to_string(),
        tree: tree_a,
    };

    let src_b = "def helper():\n    pass\n";
    let tree_b = parser.parse(src_b, None).unwrap();
    let file_b = ParsedFile {
        path: PathBuf::from("beta.py"),
        source: src_b.to_string(),
        tree: tree_b,
    };

    let src_test = "from alpha import helper\ndef test_it():\n    helper()\n";
    let tree_test = parser.parse(src_test, None).unwrap();
    let file_test = ParsedFile {
        path: PathBuf::from("test_alpha.py"),
        source: src_test.to_string(),
        tree: tree_test,
    };

    let analysis = analyze_test_refs(&[&file_a, &file_b, &file_test], None);

    assert_eq!(analysis.definitions.len(), 2, "both files define helper()");

    let unref_files: Vec<&str> = analysis
        .unreferenced
        .iter()
        .map(|d| d.file.to_str().unwrap())
        .collect();
    assert!(
        !unref_files.contains(&"alpha.py"),
        "alpha.helper should be covered (test imports from alpha)"
    );
    assert!(
        unref_files.contains(&"beta.py"),
        "beta.helper should be uncovered (no test references beta)"
    );
}

/// Exercises `disambiguate_files_graph_fallback`: when ref-based disambiguation ties
/// (both alpha and beta appear in refs), the graph picks the module imported by the
/// test that uses the name.
#[test]
fn test_disambiguate_by_graph_when_refs_tie() {
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();

    let src_a = "def helper():\n    pass\n";
    let tree_a = parser.parse(src_a, None).unwrap();
    let file_a = ParsedFile {
        path: PathBuf::from("alpha.py"),
        source: src_a.to_string(),
        tree: tree_a,
    };

    let src_b = "def helper():\n    pass\n";
    let tree_b = parser.parse(src_b, None).unwrap();
    let file_b = ParsedFile {
        path: PathBuf::from("beta.py"),
        source: src_b.to_string(),
        tree: tree_b,
    };

    let src_test_a = "from alpha import helper\ndef test_a():\n    helper()\n";
    let tree_test_a = parser.parse(src_test_a, None).unwrap();
    let file_test_a = ParsedFile {
        path: PathBuf::from("tests/test_a.py"),
        source: src_test_a.to_string(),
        tree: tree_test_a,
    };

    let src_test_b = "import beta\n";
    let tree_test_b = parser.parse(src_test_b, None).unwrap();
    let file_test_b = ParsedFile {
        path: PathBuf::from("tests/test_b.py"),
        source: src_test_b.to_string(),
        tree: tree_test_b,
    };

    let parsed: Vec<&ParsedFile> = vec![&file_a, &file_b, &file_test_a, &file_test_b];
    let graph = build_dependency_graph(&parsed);
    let analysis = analyze_test_refs(&parsed, Some(&graph));

    assert_eq!(analysis.definitions.len(), 2, "both files define helper()");

    let unref_files: Vec<&str> = analysis
        .unreferenced
        .iter()
        .map(|d| d.file.to_str().unwrap())
        .collect();
    assert!(
        !unref_files.contains(&"alpha.py"),
        "alpha.helper should be covered (graph fallback: test_a imports alpha)"
    );
    assert!(
        unref_files.contains(&"beta.py"),
        "beta.helper should be uncovered (no test imports and uses beta)"
    );
}

// ---------------------------------------------------------------------------
// collect.rs: try_add_py_def
// ---------------------------------------------------------------------------
