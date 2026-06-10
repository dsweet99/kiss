use super::*;
use std::path::Path;

// ---------------------------------------------------------------------------
// collect.rs: collect_type_refs
// ---------------------------------------------------------------------------

fn walk_for_types(
    node: tree_sitter::Node,
    source: &str,
    refs: &mut std::collections::HashSet<String>,
) {
    if node.kind() == "type" {
        super::collect::collect_type_refs(node, source, refs);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_for_types(child, source, refs);
    }
}

#[test]
fn test_collect_type_refs_captures_type_annotations() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "def foo(x: MyType) -> OtherType:\n    pass\n";
    let tree = parser.parse(src, None).unwrap();
    let root = tree.root_node();
    let mut refs = std::collections::HashSet::new();
    walk_for_types(root, src, &mut refs);
    assert!(refs.contains("MyType"), "parameter type captured");
    assert!(refs.contains("OtherType"), "return type captured");
}

// ---------------------------------------------------------------------------
// collect.rs: collect_call_target
// ---------------------------------------------------------------------------

#[test]
fn test_collect_call_target_simple_and_attribute() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "foo()\nbar.baz()\n";
    let tree = parser.parse(src, None).unwrap();
    let root = tree.root_node();
    let mut refs = std::collections::HashSet::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() == "expression_statement" {
            let call = child.child(0).unwrap();
            if call.kind() == "call"
                && let Some(func) = call.child_by_field_name("function")
            {
                super::collect::collect_call_target(func, src, &mut refs);
            }
        }
    }
    assert!(refs.contains("foo"));
    assert!(refs.contains("baz"));
    assert!(refs.contains("bar"));
}

// ---------------------------------------------------------------------------
// collect.rs: extract_import_from_binding
// ---------------------------------------------------------------------------

#[test]
fn test_extract_import_from_binding_basic() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "from mymod import foo, bar\n";
    let tree = parser.parse(src, None).unwrap();
    let root = tree.root_node();
    let import_node = root.child(0).unwrap();
    assert_eq!(import_node.kind(), "import_from_statement");
    let mut bindings = std::collections::HashMap::new();
    let mut alias_bindings = std::collections::HashMap::new();
    super::collect::extract_import_from_binding(import_node, src, &mut bindings, &mut alias_bindings);
    let names = bindings.get("mymod").expect("mymod entry");
    assert!(names.contains("foo"));
    assert!(names.contains("bar"));
}

#[test]
fn test_extract_import_from_binding_relative_skipped() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "from . import foo\n";
    let tree = parser.parse(src, None).unwrap();
    let import_node = tree.root_node().child(0).unwrap();
    let mut bindings = std::collections::HashMap::new();
    let mut alias_bindings = std::collections::HashMap::new();
    super::collect::extract_import_from_binding(import_node, src, &mut bindings, &mut alias_bindings);
    assert!(bindings.is_empty(), "relative imports are skipped");
}

#[test]
fn test_extract_import_from_binding_aliased() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "from mymod import foo as f\n";
    let tree = parser.parse(src, None).unwrap();
    let import_node = tree.root_node().child(0).unwrap();
    let mut bindings = std::collections::HashMap::new();
    let mut alias_bindings = std::collections::HashMap::new();
    super::collect::extract_import_from_binding(import_node, src, &mut bindings, &mut alias_bindings);
    let names = bindings.get("mymod").expect("mymod entry");
    assert!(
        names.contains("foo"),
        "aliased import captures original name"
    );
    assert_eq!(alias_bindings.get("f"), Some(&"foo".to_string()));
}

// ---------------------------------------------------------------------------
// collect.rs: collect_py_import_names_for_refs
// ---------------------------------------------------------------------------

#[test]
fn test_collect_py_import_names_for_refs_from_import_statement() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "import os\nimport sys\nfrom pathlib import Path\n";
    let tree = parser.parse(src, None).unwrap();
    let root = tree.root_node();
    let mut refs = std::collections::HashSet::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        super::collect::collect_py_import_names_for_refs(child, src, &mut refs);
    }
    assert!(refs.contains("os"));
    assert!(refs.contains("sys"));
    assert!(refs.contains("Path"));
    assert!(refs.contains("pathlib"));
}

// ---------------------------------------------------------------------------
// collect.rs: collect_refs_parallel on mix of test and non-test files
// ---------------------------------------------------------------------------

#[test]
fn test_collect_refs_parallel_mixed_files() {
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();

    let src_prod = "def compute():\n    pass\n";
    let tree_prod = parser.parse(src_prod, None).unwrap();
    let file_prod = ParsedFile {
        path: PathBuf::from("compute.py"),
        source: src_prod.to_string(),
        tree: tree_prod,
    };

    let src_test = "from compute import compute\ndef test_compute():\n    compute()\n";
    let tree_test = parser.parse(src_test, None).unwrap();
    let file_test = ParsedFile {
        path: PathBuf::from("test_compute.py"),
        source: src_test.to_string(),
        tree: tree_test,
    };

    let parsed: Vec<&ParsedFile> = vec![&file_prod, &file_test];
    let (defs, test_refs, usage_refs, call_refs, import_bindings, alias_bindings, per_test_usage) =
        collect_refs_parallel(&parsed, true);

    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].name, "compute");
    assert!(test_refs.contains("compute"));
    assert!(usage_refs.contains("compute"));
    assert!(call_refs.contains("compute"));
    assert!(import_bindings.get("compute").unwrap().contains("compute"));
    assert!(alias_bindings.is_empty());
    assert_eq!(per_test_usage.len(), 1);
    let (path, funcs) = &per_test_usage[0];
    assert_eq!(path, &PathBuf::from("test_compute.py"));
    assert_eq!(funcs.len(), 1);
    assert_eq!(funcs[0].0, "test_compute");
    assert!(funcs[0].1.contains("compute"));
    assert!(funcs[0].2.contains("compute"));
}

#[test]
fn test_import_with_aliased_call_is_covered() {
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();

    let src = "def some_func():\n    pass\n";
    let tree = parser.parse(src, None).unwrap();
    let file = ParsedFile {
        path: PathBuf::from("mymod.py"),
        source: src.to_string(),
        tree,
    };

    let src_test = "from mymod import some_func as sf\ndef test_it():\n    sf()\n";
    let tree_test = parser.parse(src_test, None).unwrap();
    let file_test = ParsedFile {
        path: PathBuf::from("test_mymod.py"),
        source: src_test.to_string(),
        tree: tree_test,
    };

    let analysis = analyze_test_refs(&[&file, &file_test], None);
    assert!(
        analysis.unreferenced.is_empty(),
        "some_func should be covered when called via import alias: unreferenced={:?}",
        analysis
            .unreferenced
            .iter()
            .map(|d| &d.name)
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_collect_refs_parallel_no_coverage_map() {
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();

    let src = "def helper():\n    pass\n";
    let tree = parser.parse(src, None).unwrap();
    let file = ParsedFile {
        path: PathBuf::from("mod.py"),
        source: src.to_string(),
        tree,
    };

    let src_test = "def test_it():\n    helper()\n";
    let tree_test = parser.parse(src_test, None).unwrap();
    let file_test = ParsedFile {
        path: PathBuf::from("test_mod.py"),
        source: src_test.to_string(),
        tree: tree_test,
    };

    let (_, _, _, _, _, _, per_test_usage) = collect_refs_parallel(&[&file, &file_test], false);
    assert!(
        per_test_usage.is_empty(),
        "per_test_usage empty when need_coverage_map=false"
    );
}

// ---------------------------------------------------------------------------
// collect.rs: collect_definitions with classes, methods, abstract methods, protocol
// ---------------------------------------------------------------------------

#[test]
fn test_collect_definitions_classes_and_methods() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "\
class MyClass:
    def __init__(self):
        pass
    def process(self):
        pass
    def _internal(self):
        pass

def standalone():
    pass
";
    let tree = parser.parse(src, None).unwrap();
    let mut defs = Vec::new();
    collect_definitions(
        tree.root_node(),
        src,
        Path::new("mod.py"),
        &mut defs,
        false,
        None,
    );
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"MyClass"));
    assert!(names.contains(&"__init__"));
    assert!(names.contains(&"process"));
    assert!(!names.contains(&"_internal"), "private methods excluded");
    assert!(names.contains(&"standalone"));
    let init_def = defs.iter().find(|d| d.name == "__init__").unwrap();
    assert_eq!(init_def.containing_class.as_deref(), Some("MyClass"));
    assert_eq!(init_def.kind, crate::units::CodeUnitKind::Method);
    let standalone_def = defs.iter().find(|d| d.name == "standalone").unwrap();
    assert_eq!(standalone_def.kind, crate::units::CodeUnitKind::Function);
}

#[test]
fn test_collect_definitions_abstract_methods_excluded() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "\
from abc import ABC, abstractmethod

class Base(ABC):
    @abstractmethod
    def do_work(self):
        pass

    def concrete(self):
        pass
";
    let tree = parser.parse(src, None).unwrap();
    let mut defs = Vec::new();
    collect_definitions(
        tree.root_node(),
        src,
        Path::new("base.py"),
        &mut defs,
        false,
        None,
    );
    let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    assert!(!names.contains(&"do_work"), "abstract methods excluded");
    assert!(names.contains(&"concrete"), "concrete methods included");
    assert!(names.contains(&"Base"));
}

#[test]
fn test_collect_definitions_protocol_class_excluded() {
    use crate::parsing::create_parser;
    let mut parser = create_parser().unwrap();
    let src = "class Writable(Protocol):\n    def write(self, data: str) -> None: ...\n";
    let tree = parser.parse(src, None).unwrap();
    let mut defs = Vec::new();
    collect_definitions(
        tree.root_node(),
        src,
        Path::new("ifaces.py"),
        &mut defs,
        false,
        None,
    );
    assert!(
        defs.is_empty(),
        "Protocol classes and their methods excluded"
    );
}
