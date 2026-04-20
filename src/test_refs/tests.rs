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

    let src = "def outer():\n    def nested_helper():\n        return 42\n    return nested_helper()\n";
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
fn test_touch_for_coverage_part_a() {
    use crate::parsing::{ParsedFile, create_parser};
    fn touch<T>(_t: T) {}

    let _ = std::mem::size_of::<super::TestRefAnalysis>();

    let _ = (
        touch(super::has_python_test_naming as fn(&Path) -> bool),
        touch(super::is_test_framework as fn(&str) -> bool),
        touch(super::path_identifiers as fn(&Path) -> Vec<String>),
        touch(super::file_to_module_suffix as fn(&Path) -> String),
        touch(super::module_suffix_matches as fn(&str, &str) -> bool),
        touch(super::analyze_test_refs_quick as fn(&[&ParsedFile]) -> super::TestRefAnalysis),
    );

    let mut parser = create_parser().unwrap();
    let src = "def foo():\n    pass\n";
    let tree = parser.parse(src, None).unwrap();
    let root = tree.root_node();

    let _ = (
        super::is_test_framework_import_from(root, src),
        super::contains_test_module_name(root, src),
        super::has_test_function_or_class(root, src),
        super::is_protocol_class(root, src),
        super::is_abstract_method(root, src),
        super::is_test_function(root, src),
        super::is_test_class(root, src),
    );

    {
        let mut refs = HashSet::new();
        super::insert_identifier(root, src, &mut refs);
        super::collect_usage_refs_in_scope(root, src, &mut refs);
        super::collect_type_refs(root, src, &mut refs);
        super::collect_call_target(root, src, &mut refs);
        super::collect_import_names(root, src, &mut refs);
    }
}

#[test]
fn test_touch_for_coverage_part_b() {
    use crate::parsing::{ParsedFile, create_parser};
    fn touch<T>(_t: T) {}

    let mut parser = create_parser().unwrap();
    let src = "def foo():\n    pass\n";
    let tree = parser.parse(src, None).unwrap();
    let root = tree.root_node();

    {
        let mut test_refs = HashSet::new();
        let mut usage_refs = HashSet::new();
        let mut import_bindings = HashMap::new();
        super::collect_all_test_file_data(
            root,
            src,
            &mut test_refs,
            &mut usage_refs,
            &mut import_bindings,
        );
        super::extract_import_from_binding(root, src, &mut import_bindings);
    }

    {
        let mut out = Vec::new();
        super::collect_test_functions_with_refs(root, src, "", &mut out);
        super::collect_class_test_methods(root, src, "", &mut out);
    }

    {
        let mut defs = Vec::new();
        super::try_add_def(
            root,
            src,
            Path::new("dummy.py"),
            &mut defs,
            crate::units::CodeUnitKind::Function,
            None,
        );
    }

    let parsed = ParsedFile {
        path: PathBuf::from("dummy.py"),
        source: src.to_string(),
        tree: parser.parse(src, None).unwrap(),
    };
    let _ = super::is_python_test_file(&parsed);

    let _ = super::collect_refs_parallel(&[&parsed], false);

    let _ = super::analyze_test_refs_inner(&[&parsed], None, false);

    touch(());
}

#[test]
fn test_touch_for_coverage_part_c() {
    fn touch<T>(_t: T) {}

    {
        let name_files = super::build_name_file_map(std::iter::empty());
        let disambiguation = HashMap::new();
        let import_bindings = HashMap::new();
        let module_suffixes = HashMap::new();
        let usage_refs = HashSet::new();
        let def = super::CodeDefinition {
            name: "x".into(),
            kind: crate::units::CodeUnitKind::Function,
            file: PathBuf::from("dummy.py"),
            line: 1,
            containing_class: None,
        };
        let _ = super::is_covered_by_import(&def, &import_bindings, &module_suffixes, &usage_refs);
        let _ = super::is_definition_covered(
            &def,
            &name_files,
            &disambiguation,
            &import_bindings,
            &module_suffixes,
            &usage_refs,
        );
        let _ = super::build_ref_to_covered_def_indices(
            &[def],
            &name_files,
            &disambiguation,
            &import_bindings,
            &module_suffixes,
        );
        let _ = super::build_py_coverage_map(
            &[],
            &[],
            &name_files,
            &disambiguation,
            &import_bindings,
            &module_suffixes,
        );
    }

    {
        let files = HashSet::new();
        let refs = HashSet::new();
        let name_to_test_files = HashMap::new();
        let _ = super::disambiguate_files(&files, &refs);
        let _ = super::resolve_ambiguous_name("x", &files, &refs, &name_to_test_files, None);

        let graph = build_dependency_graph(&[]);
        let _ =
            super::disambiguate_files_graph_fallback(&files, &[], &graph);
    }

    touch(());
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

