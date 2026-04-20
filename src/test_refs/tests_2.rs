use super::*;

#[test]
fn test_file_stem_collision_no_false_positive() {
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();

    let src_utils = "def parse():\n    pass\n";
    let tree_utils = parser.parse(src_utils, None).unwrap();
    let file_utils = ParsedFile {
        path: PathBuf::from("utils.py"),
        source: src_utils.to_string(),
        tree: tree_utils,
    };

    let src_helpers = "def parse():\n    pass\n";
    let tree_helpers = parser.parse(src_helpers, None).unwrap();
    let file_helpers = ParsedFile {
        path: PathBuf::from("helpers.py"),
        source: src_helpers.to_string(),
        tree: tree_helpers,
    };

    let src_test = "from utils import parse\nimport helpers\ndef test_it():\n    parse()\n    helpers.do_stuff()\n";
    let tree_test = parser.parse(src_test, None).unwrap();
    let file_test = ParsedFile {
        path: PathBuf::from("test_stuff.py"),
        source: src_test.to_string(),
        tree: tree_test,
    };

    let analysis = analyze_test_refs(&[&file_utils, &file_helpers, &file_test], None);

    let unref_files: Vec<&str> = analysis
        .unreferenced
        .iter()
        .map(|d| d.file.to_str().unwrap())
        .collect();
    assert!(
        unref_files.contains(&"helpers.py"),
        "helpers.parse should be uncovered (test doesn't exercise it): unreferenced={unref_files:?}"
    );
}

#[test]
fn test_same_stem_as_function_name_different_dirs() {
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();

    let src = "def some_name():\n    pass\n";

    let tree_1 = parser.parse(src, None).unwrap();
    let file_1 = ParsedFile {
        path: PathBuf::from("sub_dir_1/some_name.py"),
        source: src.to_string(),
        tree: tree_1,
    };

    let tree_2 = parser.parse(src, None).unwrap();
    let file_2 = ParsedFile {
        path: PathBuf::from("sub_dir_2/some_name.py"),
        source: src.to_string(),
        tree: tree_2,
    };

    let src_test =
        "from sub_dir_1.some_name import some_name\ndef test_it():\n    some_name()\n";
    let tree_test = parser.parse(src_test, None).unwrap();
    let file_test = ParsedFile {
        path: PathBuf::from("test_stuff.py"),
        source: src_test.to_string(),
        tree: tree_test,
    };

    let analysis = analyze_test_refs(&[&file_1, &file_2, &file_test], None);

    let unref: Vec<_> = analysis
        .unreferenced
        .iter()
        .map(|d| d.file.to_str().unwrap())
        .collect();
    assert!(
        !unref.contains(&"sub_dir_1/some_name.py"),
        "sub_dir_1/some_name::some_name should be covered (explicitly imported and called): unreferenced={unref:?}"
    );
    assert!(
        unref.contains(&"sub_dir_2/some_name.py"),
        "sub_dir_2/some_name::some_name should be uncovered: unreferenced={unref:?}"
    );
}

#[test]
fn test_import_module_without_from_falls_back() {
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();

    let src = "def func():\n    pass\n";

    let tree_1 = parser.parse(src, None).unwrap();
    let file_1 = ParsedFile {
        path: PathBuf::from("alpha.py"),
        source: src.to_string(),
        tree: tree_1,
    };

    let tree_2 = parser.parse(src, None).unwrap();
    let file_2 = ParsedFile {
        path: PathBuf::from("beta.py"),
        source: src.to_string(),
        tree: tree_2,
    };

    let src_test = "import alpha\ndef test_it():\n    alpha.func()\n";
    let tree_test = parser.parse(src_test, None).unwrap();
    let file_test = ParsedFile {
        path: PathBuf::from("test_stuff.py"),
        source: src_test.to_string(),
        tree: tree_test,
    };

    let analysis = analyze_test_refs(&[&file_1, &file_2, &file_test], None);

    let unref: Vec<_> = analysis
        .unreferenced
        .iter()
        .map(|d| d.file.to_str().unwrap())
        .collect();
    assert!(
        !unref.contains(&"alpha.py"),
        "`import alpha; alpha.func()` should cover alpha.func via fallback: unreferenced={unref:?}"
    );
    assert!(
        unref.contains(&"beta.py"),
        "beta.func should be uncovered: unreferenced={unref:?}"
    );
}

#[test]
fn test_relative_import_falls_back_to_flat_refs() {
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();

    let src = "def helper():\n    pass\n";
    let tree = parser.parse(src, None).unwrap();
    let file = ParsedFile {
        path: PathBuf::from("mymod.py"),
        source: src.to_string(),
        tree,
    };

    let src_test = "from . import mymod\ndef test_it():\n    mymod.helper()\n";
    let tree_test = parser.parse(src_test, None).unwrap();
    let file_test = ParsedFile {
        path: PathBuf::from("test_mymod.py"),
        source: src_test.to_string(),
        tree: tree_test,
    };

    let analysis = analyze_test_refs(&[&file, &file_test], None);

    assert!(
        analysis.unreferenced.is_empty(),
        "relative import should fall back to flat refs and cover helper: unreferenced={:?}",
        analysis
            .unreferenced
            .iter()
            .map(|d| &d.name)
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_import_without_call_not_covered() {
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();

    let src = "def some_func():\n    pass\n";
    let tree = parser.parse(src, None).unwrap();
    let file = ParsedFile {
        path: PathBuf::from("mymod.py"),
        source: src.to_string(),
        tree,
    };

    let src_test = "from mymod import some_func\ndef test_it():\n    pass\n";
    let tree_test = parser.parse(src_test, None).unwrap();
    let file_test = ParsedFile {
        path: PathBuf::from("test_mymod.py"),
        source: src_test.to_string(),
        tree: tree_test,
    };

    let analysis = analyze_test_refs(&[&file, &file_test], None);
    assert!(
        !analysis.unreferenced.is_empty(),
        "some_func should be uncovered (imported but never called)"
    );
}

#[test]
fn test_import_with_call_is_covered() {
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();

    let src = "def some_func():\n    pass\n";
    let tree = parser.parse(src, None).unwrap();
    let file = ParsedFile {
        path: PathBuf::from("mymod.py"),
        source: src.to_string(),
        tree,
    };

    let src_test = "from mymod import some_func\ndef test_it():\n    some_func()\n";
    let tree_test = parser.parse(src_test, None).unwrap();
    let file_test = ParsedFile {
        path: PathBuf::from("test_mymod.py"),
        source: src_test.to_string(),
        tree: tree_test,
    };

    let analysis = analyze_test_refs(&[&file, &file_test], None);
    assert!(
        analysis.unreferenced.is_empty(),
        "some_func should be covered (imported AND called): unreferenced={:?}",
        analysis
            .unreferenced
            .iter()
            .map(|d| &d.name)
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_class_import_without_use_not_covered() {
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();

    let src = "class MyClass:\n    def __init__(self):\n        pass\n    def process(self):\n        pass\n";
    let tree = parser.parse(src, None).unwrap();
    let file = ParsedFile {
        path: PathBuf::from("mymod.py"),
        source: src.to_string(),
        tree,
    };

    let src_test = "from mymod import MyClass\ndef test_it():\n    pass\n";
    let tree_test = parser.parse(src_test, None).unwrap();
    let file_test = ParsedFile {
        path: PathBuf::from("test_mymod.py"),
        source: src_test.to_string(),
        tree: tree_test,
    };

    let analysis = analyze_test_refs(&[&file, &file_test], None);
    let unref_names: Vec<&str> = analysis
        .unreferenced
        .iter()
        .map(|d| d.name.as_str())
        .collect();
    assert!(
        unref_names.contains(&"__init__"),
        "MyClass.__init__ should be uncovered (class imported but never used): unreferenced={unref_names:?}"
    );
    assert!(
        unref_names.contains(&"process"),
        "MyClass.process should be uncovered (class imported but never used): unreferenced={unref_names:?}"
    );
}

#[test]
fn test_protocol_class_excluded_from_coverage() {
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();

    let src = "from typing import Protocol\n\nclass Readable(Protocol):\n    def read(self) -> str: ...\n";
    let tree = parser.parse(src, None).unwrap();
    let file = ParsedFile {
        path: PathBuf::from("interfaces.py"),
        source: src.to_string(),
        tree,
    };

    let src_test = "def test_placeholder():\n    pass\n";
    let tree_test = parser.parse(src_test, None).unwrap();
    let file_test = ParsedFile {
        path: PathBuf::from("test_interfaces.py"),
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
        !def_names.contains(&"Readable"),
        "Protocol class should not be tracked for coverage: definitions={def_names:?}"
    );
    assert!(
        !def_names.contains(&"read"),
        "Protocol method should not be tracked for coverage: definitions={def_names:?}"
    );
}

#[test]
fn test_abstract_methods_excluded_from_coverage() {
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();

    let src = "from abc import ABC, abstractmethod\n\nclass MyBase(ABC):\n    @abstractmethod\n    def process(self):\n        pass\n\n    def concrete(self):\n        return 42\n";
    let tree = parser.parse(src, None).unwrap();
    let file = ParsedFile {
        path: PathBuf::from("base.py"),
        source: src.to_string(),
        tree,
    };

    let src_test = "def test_placeholder():\n    pass\n";
    let tree_test = parser.parse(src_test, None).unwrap();
    let file_test = ParsedFile {
        path: PathBuf::from("test_base.py"),
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
        !def_names.contains(&"process"),
        "Abstract method should not be tracked for coverage: definitions={def_names:?}"
    );
    assert!(
        def_names.contains(&"concrete"),
        "Concrete method should still be tracked: definitions={def_names:?}"
    );
}

#[test]
fn test_coverage_map_one_function_covered_by_two_tests() {
    use crate::parsing::{ParsedFile, create_parser};
    let mut parser = create_parser().unwrap();

    let src = "def parse(x):\n    return int(x or 0)\n";
    let tree = parser.parse(src, None).unwrap();
    let file = ParsedFile {
        path: PathBuf::from("utils.py"),
        source: src.to_string(),
        tree,
    };

    let src_test = "from utils import parse\n\ndef test_parse_empty():\n    assert parse('') == 0\n\ndef test_parse_valid():\n    assert parse('42') == 42\n";
    let tree_test = parser.parse(src_test, None).unwrap();
    let file_test = ParsedFile {
        path: PathBuf::from("test_utils.py"),
        source: src_test.to_string(),
        tree: tree_test,
    };

    let analysis = analyze_test_refs(&[&file, &file_test], None);
    let key = (PathBuf::from("utils.py"), "parse".to_string());
    let covering = analysis.coverage_map.get(&key).expect("coverage_map should have parse");
    let test_ids: Vec<String> = covering
        .iter()
        .map(|(_, func)| func.clone())
        .collect();
    assert!(
        test_ids.contains(&"test_parse_empty".to_string()),
        "coverage_map should list test_parse_empty, got: {test_ids:?}"
    );
    assert!(
        test_ids.contains(&"test_parse_valid".to_string()),
        "coverage_map should list test_parse_valid, got: {test_ids:?}"
    );
    assert_eq!(covering.len(), 2);
}

