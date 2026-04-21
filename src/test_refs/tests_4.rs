use super::*;

// ---------------------------------------------------------------------------
// coverage.rs: is_definition_covered
// ---------------------------------------------------------------------------

#[test]
fn test_is_definition_covered_unique_name() {
    let def = CodeDefinition {
        name: "unique_func".to_string(),
        kind: crate::units::CodeUnitKind::Function,
        file: PathBuf::from("mod.py"),
        line: 1,
        containing_class: None,
    };
    let mut name_files = HashMap::new();
    name_files
        .entry("unique_func".to_string())
        .or_insert_with(HashSet::new)
        .insert(PathBuf::from("mod.py"));
    let disambiguation = HashMap::new();
    let import_bindings = HashMap::new();
    let module_suffixes = HashMap::new();
    let mut usage_refs = HashSet::new();
    usage_refs.insert("unique_func".to_string());

    assert!(is_definition_covered(
        &def,
        &name_files,
        &disambiguation,
        &import_bindings,
        &module_suffixes,
        &usage_refs,
    ));
}

#[test]
fn test_is_definition_covered_by_containing_class() {
    let def = CodeDefinition {
        name: "process".to_string(),
        kind: crate::units::CodeUnitKind::Method,
        file: PathBuf::from("mod.py"),
        line: 5,
        containing_class: Some("MyClass".to_string()),
    };
    let name_files = HashMap::new();
    let disambiguation = HashMap::new();
    let import_bindings = HashMap::new();
    let module_suffixes = HashMap::new();
    let mut usage_refs = HashSet::new();
    usage_refs.insert("MyClass".to_string());

    assert!(is_definition_covered(
        &def,
        &name_files,
        &disambiguation,
        &import_bindings,
        &module_suffixes,
        &usage_refs,
    ));
}

#[test]
fn test_is_definition_covered_disambiguation_winner() {
    let def = CodeDefinition {
        name: "dup".to_string(),
        kind: crate::units::CodeUnitKind::Function,
        file: PathBuf::from("alpha.py"),
        line: 1,
        containing_class: None,
    };
    let mut name_files = HashMap::new();
    let mut files = HashSet::new();
    files.insert(PathBuf::from("alpha.py"));
    files.insert(PathBuf::from("beta.py"));
    name_files.insert("dup".to_string(), files);
    let mut disambiguation = HashMap::new();
    disambiguation.insert("dup".to_string(), PathBuf::from("alpha.py"));
    let import_bindings = HashMap::new();
    let module_suffixes = HashMap::new();
    let mut usage_refs = HashSet::new();
    usage_refs.insert("dup".to_string());

    assert!(is_definition_covered(
        &def,
        &name_files,
        &disambiguation,
        &import_bindings,
        &module_suffixes,
        &usage_refs,
    ));

    let def_beta = CodeDefinition {
        name: "dup".to_string(),
        kind: crate::units::CodeUnitKind::Function,
        file: PathBuf::from("beta.py"),
        line: 1,
        containing_class: None,
    };
    assert!(!is_definition_covered(
        &def_beta,
        &name_files,
        &disambiguation,
        &import_bindings,
        &module_suffixes,
        &usage_refs,
    ));
}

// ---------------------------------------------------------------------------
// coverage.rs: build_ref_to_covered_def_indices
// ---------------------------------------------------------------------------

#[test]
fn test_build_ref_to_covered_def_indices_unique_and_class() {
    use super::coverage::build_ref_to_covered_def_indices;
    let definitions = vec![
        CodeDefinition {
            name: "foo".to_string(),
            kind: crate::units::CodeUnitKind::Function,
            file: PathBuf::from("mod.py"),
            line: 1,
            containing_class: None,
        },
        CodeDefinition {
            name: "bar".to_string(),
            kind: crate::units::CodeUnitKind::Method,
            file: PathBuf::from("mod.py"),
            line: 5,
            containing_class: Some("Cls".to_string()),
        },
    ];
    let mut name_files = HashMap::new();
    name_files.entry("foo".to_string()).or_insert_with(HashSet::new).insert(PathBuf::from("mod.py"));
    name_files.entry("bar".to_string()).or_insert_with(HashSet::new).insert(PathBuf::from("mod.py"));
    let disambiguation = HashMap::new();
    let import_bindings = HashMap::new();
    let module_suffixes = HashMap::new();

    let map = build_ref_to_covered_def_indices(
        &definitions,
        &name_files,
        &disambiguation,
        &import_bindings,
        &module_suffixes,
    );
    assert!(map.get("foo").unwrap().contains(&0));
    assert!(map.get("bar").unwrap().contains(&1));
    assert!(map.get("Cls").unwrap().contains(&1), "class name maps to method index");
}

// ---------------------------------------------------------------------------
// coverage.rs: build_py_coverage_map
// ---------------------------------------------------------------------------

#[test]
fn test_build_py_coverage_map_per_test_tracking() {
    let definitions = vec![
        CodeDefinition {
            name: "run".to_string(),
            kind: crate::units::CodeUnitKind::Function,
            file: PathBuf::from("engine.py"),
            line: 1,
            containing_class: None,
        },
    ];
    let mut name_files: HashMap<String, HashSet<PathBuf>> = HashMap::new();
    name_files.entry("run".to_string()).or_default().insert(PathBuf::from("engine.py"));
    let disambiguation = HashMap::new();
    let import_bindings = HashMap::new();
    let module_suffixes = HashMap::new();

    let mut refs_a = HashSet::new();
    refs_a.insert("run".to_string());
    let mut refs_b = HashSet::new();
    refs_b.insert("other".to_string());
    let per_test_usage: super::PerTestUsage = vec![(
        PathBuf::from("test_engine.py"),
        vec![
            ("test_a".to_string(), refs_a),
            ("test_b".to_string(), refs_b),
        ],
    )];

    let cov = build_py_coverage_map(
        &definitions,
        &per_test_usage,
        &name_files,
        &disambiguation,
        &import_bindings,
        &module_suffixes,
    );

    let key = (PathBuf::from("engine.py"), "run".to_string());
    let tests = cov.get(&key).expect("run should be covered");
    assert_eq!(tests.len(), 1);
    assert_eq!(tests[0].1, "test_a");
}

// ---------------------------------------------------------------------------
// detection.rs: has_python_test_naming
// ---------------------------------------------------------------------------

#[test]
fn test_has_python_test_naming_various() {
    use super::detection::has_python_test_naming;
    use std::path::Path;
    assert!(has_python_test_naming(Path::new("test_foo.py")));
    assert!(has_python_test_naming(Path::new("foo_test.py")));
    assert!(has_python_test_naming(Path::new("conftest.py")));
    assert!(!has_python_test_naming(Path::new("testfoo.py")));
    assert!(!has_python_test_naming(Path::new("foo.py")));
    assert!(!has_python_test_naming(Path::new("test_foo.txt")));
}

// ---------------------------------------------------------------------------
// detection.rs: is_test_framework, is_test_framework_import_from
// ---------------------------------------------------------------------------

#[test]
fn test_is_test_framework_known_names() {
    use super::detection::is_test_framework;
    assert!(is_test_framework("pytest"));
    assert!(is_test_framework("unittest"));
    assert!(is_test_framework("pytest.fixtures"));
    assert!(is_test_framework("unittest.mock"));
    assert!(!is_test_framework("requests"));
    assert!(!is_test_framework("pytestx"));
}

#[test]
fn test_is_test_framework_import_from_node() {
    use super::detection::is_test_framework_import_from;
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "from pytest import fixture\n";
    let tree = parser.parse(src, None).unwrap();
    let import_from = tree.root_node().child(0).unwrap();
    assert!(is_test_framework_import_from(import_from, src));

    let src2 = "from os import path\n";
    let tree2 = parser.parse(src2, None).unwrap();
    let import_from2 = tree2.root_node().child(0).unwrap();
    assert!(!is_test_framework_import_from(import_from2, src2));
}

// ---------------------------------------------------------------------------
// detection.rs: contains_test_module_name
// ---------------------------------------------------------------------------

#[test]
fn test_contains_test_module_name_import_statement() {
    use super::detection::contains_test_module_name;
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "import pytest\n";
    let tree = parser.parse(src, None).unwrap();
    let import_node = tree.root_node().child(0).unwrap();
    assert!(contains_test_module_name(import_node, src));

    let src2 = "import os\n";
    let tree2 = parser.parse(src2, None).unwrap();
    let import_node2 = tree2.root_node().child(0).unwrap();
    assert!(!contains_test_module_name(import_node2, src2));
}

// ---------------------------------------------------------------------------
// detection.rs: has_test_function_or_class
// ---------------------------------------------------------------------------

#[test]
fn test_has_test_function_or_class_detects_test_func() {
    use super::detection::has_test_function_or_class;
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "def test_something():\n    pass\n";
    let tree = parser.parse(src, None).unwrap();
    assert!(has_test_function_or_class(tree.root_node(), src));

    let src2 = "def helper():\n    pass\n";
    let tree2 = parser.parse(src2, None).unwrap();
    assert!(!has_test_function_or_class(tree2.root_node(), src2));
}

#[test]
fn test_has_test_function_or_class_detects_test_class() {
    use super::detection::has_test_function_or_class;
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "class TestSuite:\n    def test_one(self):\n        pass\n";
    let tree = parser.parse(src, None).unwrap();
    assert!(has_test_function_or_class(tree.root_node(), src));
}

// ---------------------------------------------------------------------------
// detection.rs: is_python_test_file
// ---------------------------------------------------------------------------

fn make_python_test_file_check(path: &str, src: &str) -> bool {
    use super::detection::is_python_test_file;
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();
    let tree = parser.parse(src, None).unwrap();
    let file = ParsedFile {
        path: PathBuf::from(path),
        source: src.to_string(),
        tree,
    };
    is_python_test_file(&file)
}

#[test]
fn test_is_python_test_file_by_name() {
    assert!(make_python_test_file_check("test_foo.py", "x = 1\n"));
}

#[test]
fn test_is_python_test_file_in_tests_dir() {
    assert!(make_python_test_file_check("tests/helpers.py", "x = 1\n"));
}

#[test]
fn test_is_python_test_file_by_content() {
    assert!(make_python_test_file_check("src/checker.py", "import pytest\ndef test_x():\n    pass\n"));
}

#[test]
fn test_is_python_test_file_regular_file() {
    assert!(!make_python_test_file_check("src/utils.py", "def helper():\n    pass\n"));
}
