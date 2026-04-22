use super::tests::new_graph;
use crate::graph::{
    DependencyGraph, build_dependency_graph, build_dependency_graph_from_import_lists,
    cycle_size_violation, is_test_module, qualified_module_name, resolve_import,
};
use crate::parsing::ParsedFile;
use crate::parsing::create_parser;
use std::collections::HashMap;
use std::path::PathBuf;

fn build_temp_pkg_graph(importer_code: &str) -> DependencyGraph {
    use crate::parsing::parse_file;
    use std::fs;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    let pkg = src.join("pkg");
    fs::create_dir_all(&pkg).unwrap();

    fs::write(pkg.join("__init__.py"), "").unwrap();
    fs::write(pkg.join("target.py"), "def do_work():\n    return 42\n").unwrap();
    fs::write(pkg.join("importer.py"), importer_code).unwrap();

    let importer = {
        let mut parser = create_parser().unwrap();
        parse_file(&mut parser, &pkg.join("importer.py")).unwrap()
    };
    let target = {
        let mut parser = create_parser().unwrap();
        parse_file(&mut parser, &pkg.join("target.py")).unwrap()
    };
    let init = {
        let mut parser = create_parser().unwrap();
        parse_file(&mut parser, &pkg.join("__init__.py")).unwrap()
    };

    let parsed_files: Vec<&ParsedFile> = vec![&importer, &target, &init];
    build_dependency_graph(&parsed_files)
}

#[test]
fn test_build_dependency_graph_creates_edge_for_from_dot_import() {
    let g =
        build_temp_pkg_graph("def f():\n    from . import target\n    return target.do_work()\n");

    let m_importer = g.module_metrics("pkg.importer");
    let m_target = g.module_metrics("pkg.target");
    assert!(
        m_importer.fan_out >= 1 && m_target.fan_in >= 1,
        "Expected edge pkg.importer -> pkg.target (fan_out/fan_in >= 1); got importer={m_importer:?} target={m_target:?}"
    );
}

#[test]
fn test_from_import_adds_submodule_candidate_when_internal() {
    let g =
        build_temp_pkg_graph("def f():\n    from pkg import target\n    return target.do_work()\n");

    let m_importer = g.module_metrics("pkg.importer");
    let m_target = g.module_metrics("pkg.target");
    assert!(
        m_importer.fan_out >= 1 && m_target.fan_in >= 1,
        "Expected `from pkg import target` to create an internal edge to pkg.target; got importer={m_importer:?} target={m_target:?}"
    );
}

#[test]
fn test_qualified_and_bare_module_names() {
    use crate::graph::bare_module_name;
    use std::path::Path;
    assert_eq!(
        qualified_module_name(Path::new("src/attr/exceptions.py")),
        "attr.exceptions"
    );
    assert_eq!(
        qualified_module_name(Path::new("src/pkg/__init__.py")),
        "pkg",
        "__init__.py should map to the package module name"
    );
    assert_eq!(
        qualified_module_name(Path::new("click/utils.py")),
        "click.utils"
    );
    assert_eq!(qualified_module_name(Path::new("utils.py")), "utils");
    assert_eq!(qualified_module_name(Path::new("./foo.py")), "foo");

    assert_eq!(
        bare_module_name(Path::new("src/attr/exceptions.py")),
        "exceptions"
    );
    assert_eq!(
        bare_module_name(Path::new("src/pkg/__init__.py")),
        "pkg",
        "__init__.py should use the containing directory as bare name"
    );
    assert_eq!(bare_module_name(Path::new("click/utils.py")), "utils");
}

#[test]
fn test_resolve_import() {
    let mut bare_to_qualified: HashMap<String, Vec<String>> = HashMap::new();
    bare_to_qualified.insert(
        "exceptions".into(),
        vec!["attr.exceptions".into(), "click.exceptions".into()],
    );
    bare_to_qualified.insert("utils".into(), vec!["click.utils".into()]);

    assert_eq!(
        resolve_import("utils", Some("click"), &bare_to_qualified),
        vec!["click.utils".to_string()]
    );

    assert_eq!(
        resolve_import("exceptions", Some("attr"), &bare_to_qualified),
        vec!["attr.exceptions".to_string()]
    );
    assert_eq!(
        resolve_import("exceptions", Some("click"), &bare_to_qualified),
        vec!["click.exceptions".to_string()]
    );

    assert!(resolve_import("exceptions", Some("httpx"), &bare_to_qualified).is_empty());

    assert!(resolve_import("unknown", Some("attr"), &bare_to_qualified).is_empty());
}

// === Bug-hunting tests ===

#[test]
fn test_indirect_deps_in_cycle() {
    let mut g = new_graph();
    g.add_dependency("a", "b");
    g.add_dependency("b", "a");
    let metrics = g.module_metrics("a");
    assert_eq!(
        metrics.indirect_dependencies, 0,
        "Module 'a' has fan_out=1 and total_reachable=1, so indirect should be 0 (got {})",
        metrics.indirect_dependencies
    );
}

#[test]
fn test_test_importers_of_returns_test_modules_that_import_target() {
    let files = vec![
        (PathBuf::from("src/utils.py"), vec![]),
        (
            PathBuf::from("tests/test_utils.py"),
            vec!["utils".to_string()],
        ),
        (PathBuf::from("other/helper.py"), vec!["utils".to_string()]),
    ];
    let graph = build_dependency_graph_from_import_lists(&files);
    let importers = graph.test_importers_of("utils");
    assert!(
        importers.iter().any(|m| m.contains("test_utils")),
        "test_importers_of should return test module that imports utils, got {importers:?}"
    );
    assert!(
        !importers.iter().any(|m| m.contains("helper")),
        "test_importers_of should not return non-test importers, got {importers:?}"
    );
}

#[test]
fn test_is_test_module_singular_test_dir() {
    let mut g = DependencyGraph::new();
    g.path_to_module.insert(
        std::path::PathBuf::from("test/helpers.py"),
        "test.helpers".into(),
    );
    g.paths.insert(
        "test.helpers".into(),
        std::path::PathBuf::from("test/helpers.py"),
    );
    assert!(
        is_test_module(&g, "test.helpers"),
        "Modules under test/ (singular) should be recognized as test modules"
    );
}

#[test]
fn test_touch_importinfo_and_push_import_name_segments() {
    use crate::graph::{ImportInfo, push_import_name_segments};
    let _ = ImportInfo {
        from_qualified: "a.b".into(),
        from_parent_module: Some("a".into()),
        imports: vec!["os".into()],
    };
    let mut parser = create_parser().unwrap();
    let tree = parser.parse("import os", None).unwrap();
    let node = tree.root_node().child(0).unwrap();
    let mut imports = Vec::new();
    let dotted = node.child(1).unwrap();
    push_import_name_segments(dotted, "import os", &mut imports);
    assert!(imports.contains(&"os".into()));
}

#[test]
fn test_cycle_size_violation_suggestion_does_not_claim_unimplemented_min_cut() {
    let mut g = DependencyGraph::new();
    g.add_dependency("a", "b");
    g.add_dependency("b", "c");
    g.add_dependency("c", "a");
    let v = cycle_size_violation(&g, &["a".to_string(), "b".to_string(), "c".to_string()], 1);
    assert!(
        !v.suggestion.to_lowercase().contains("min-cut"),
        "suggestion should not reference min-cut without that analysis; got: {}",
        v.suggestion
    );
}
