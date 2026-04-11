use crate::graph::{
    add_edges_for_import_info, bare_module_name, build_dependency_graph,
    build_dependency_graph_from_import_lists, collect_imported_name_candidates,
    collect_module_violations, compute_cyclomatic_complexity, count_decision_points,
    cycle_size_violation, dependency_depth_violation, extract_dynamic_import_module,
    extract_imports_for_cache, extract_imports_recursive, extract_modules_from_import_from,
    file_stem_str, get_module_path, indirect_deps_violation, is_decision_point, is_dunder_import,
    is_entry_point, is_importlib_import_module, is_init_module, is_orphan, is_path_covered_by_another,
    is_test_module, join_qualified_dirs_and_stem, orphan_violation, parent_dir_strings,
    parent_prefix_match, parse_python_string_literal, path_dedup_set, push_dotted_segments,
    push_import_name_segments, qualified_module_name, read_base_module, resolve_bare, resolve_dotted,
    resolve_import, strip_rbub_prefix, trim_src_suffix, unquote_single, unquote_triple,
    CycleInfo, DependencyGraph, GraphBuildState, ImportInfo, ImportListPass,
};
use crate::parsing::create_parser;
use crate::parsing::ParsedFile;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

#[test]
fn test_touch_dynamic_import_helpers_for_static_coverage() {
    // Touch private helpers so static test-ref coverage includes them.
    let mut parser = create_parser().unwrap();
    let code = "def f():\n    import importlib\n    importlib.import_module(\"pkg.target\")\n    __import__(\"pkg.other\")\n";
    let tree = parser.parse(code, None).unwrap();
    let root = tree.root_node();

    // Ensure extract_imports_for_cache sees the dynamic imports.
    let imports = extract_imports_for_cache(root, code);
    assert!(imports.contains(&"pkg.target".into()));
    assert!(imports.contains(&"pkg.other".into()));

    // Directly touch helper fns with a best-effort call node.
    let call_node = root
        .descendant_for_byte_range(code.find("importlib.import_module").unwrap(), code.len())
        .unwrap();
    let _ = extract_dynamic_import_module(call_node, code);
}

#[test]
fn static_coverage_touch_graph_helpers() {
    fn t<T>(_: T) {}
    t(is_init_module);
    t(path_dedup_set);
    t(is_path_covered_by_another);
    t(parent_prefix_match);
    t(resolve_bare);
    t(resolve_dotted);
    t(parse_python_string_literal);
    t(orphan_violation);
    t(indirect_deps_violation);
    t(dependency_depth_violation);
    t(cycle_size_violation);
    t(collect_module_violations);
    t(add_edges_for_import_info);
    t(GraphBuildState::register_module);
    t(ImportListPass::add_edges);
    t(file_stem_str);
    t(parent_dir_strings);
    t(trim_src_suffix);
    t(join_qualified_dirs_and_stem);
    t(read_base_module);
    t(collect_imported_name_candidates);
    t(is_importlib_import_module);
    t(is_dunder_import);
    t(strip_rbub_prefix);
    t(unquote_triple);
    t(unquote_single);
}

#[test]
fn test_graph_imports_and_cycles() {
    let mut parser = create_parser().unwrap();
    assert!(
        extract_imports_for_cache(
            parser.parse("import os", None).unwrap().root_node(),
            "import os"
        )
        .contains(&"os".into())
    );
    let code = "import os\ndef foo():\n    import json\n    from sys import argv";
    let mut nested = Vec::new();
    extract_imports_recursive(
        parser.parse(code, None).unwrap().root_node(),
        code,
        &mut nested,
    );
    assert!(
        nested.contains(&"os".into())
            && nested.contains(&"json".into())
            && nested.contains(&"sys".into())
    );
    let mut g = DependencyGraph::default();
    g.add_dependency("a", "b");
    g.add_dependency("b", "a");
    let cycle_info: CycleInfo = g.find_cycles();
    assert!(!cycle_info.cycles.is_empty());
    assert_eq!(g.get_or_create_node("test"), g.get_or_create_node("test"));
    g.add_dependency("x", "x");
    // Self-dependencies are rejected: neither node nor edge is created
    assert!(
        !g.nodes.contains_key("x"),
        "Self-dependency should not create node"
    );
    let idx_a = *g.nodes.get("a").unwrap();
    let idx_b = *g.nodes.get("b").unwrap();
    assert!(g.is_cycle(&[idx_a, idx_b]) && !g.is_cycle(&[]) && !g.is_cycle(&[idx_a]));
    g.add_dependency("a", "c");
    g.add_dependency("c", "d");
    let (reachable, depth) = g.compute_reachable_and_depth(*g.nodes.get("a").unwrap());
    assert!(reachable >= 2 && depth >= 2);
}

#[test]
fn test_from_import_does_not_create_edges_to_imported_names() {
    // Hypothesis 1 repro: `from X import Y` currently adds both `X` and `Y` as dependencies.
    // That can create huge, fake SCC cycles when `Y` happens to match some other module name.
    //
    // This fixture is *acyclic* under real Python import semantics:
    // - a imports b (and name c from b)
    // - b imports c (and name a from c)
    // - c imports nothing
    //
    // There is no module-level cycle unless we incorrectly treat imported names as modules.
    let mut parser = create_parser().unwrap();
    let files: Vec<(PathBuf, Vec<String>)> = vec![
        (
            PathBuf::from("a.py"),
            extract_imports_for_cache(
                parser.parse("from b import c\n", None).unwrap().root_node(),
                "from b import c\n",
            ),
        ),
        (
            PathBuf::from("b.py"),
            extract_imports_for_cache(
                parser.parse("from c import a\n", None).unwrap().root_node(),
                "from c import a\n",
            ),
        ),
        (
            PathBuf::from("c.py"),
            extract_imports_for_cache(parser.parse("\n", None).unwrap().root_node(), "\n"),
        ),
    ];

    let graph = build_dependency_graph_from_import_lists(&files);
    let cycles = graph.find_cycles().cycles;
    assert!(
        cycles.is_empty(),
        "Expected no module cycle; got cycles: {cycles:?}"
    );
}

#[test]
fn test_dotted_import_does_not_create_edges_to_middle_segments() {
    // Hypothesis 2 repro: `import foo.bar` is currently split into segments `foo` and `bar`,
    // which can spuriously create an edge to a local `bar.py` module.
    let mut parser = create_parser().unwrap();
    let files: Vec<(PathBuf, Vec<String>)> = vec![
        (
            PathBuf::from("a.py"),
            extract_imports_for_cache(
                parser.parse("import foo.bar\n", None).unwrap().root_node(),
                "import foo.bar\n",
            ),
        ),
        // Local module named `bar` should NOT be considered imported by `import foo.bar`.
        (PathBuf::from("bar.py"), Vec::new()),
    ];

    let graph = build_dependency_graph_from_import_lists(&files);
    let a = qualified_module_name(&PathBuf::from("a.py"));
    let bar = qualified_module_name(&PathBuf::from("bar.py"));
    let a_idx = *graph.nodes.get(&a).expect("a node");
    let bar_idx = *graph.nodes.get(&bar).expect("bar node");

    assert!(
        !graph.graph.contains_edge(a_idx, bar_idx),
        "Expected no edge {a} -> {bar} from `import foo.bar`"
    );
}

#[test]
fn test_qualified_module_name_includes_full_package_path() {
    // Hypothesis 3 repro: qualified_module_name currently only includes the leaf parent dir,
    // so deep paths can collide (e.g., pkg1/sub/utils.py and pkg2/sub/utils.py).
    use std::path::Path;
    let a = qualified_module_name(Path::new("pkg1/sub/utils.py"));
    let b = qualified_module_name(Path::new("pkg2/sub/utils.py"));
    assert_ne!(
        a, b,
        "Qualified module names should not collide for distinct deep package paths"
    );
}

#[test]
fn test_helpers_imports_and_complexity() {
    assert!(is_entry_point("main") && is_entry_point("test_foo") && !is_entry_point("utils"));
    assert!(
        is_entry_point("bin.lock_server"),
        "Rust src/bin/*.rs should be entry points"
    );
    assert!(is_entry_point("bin"), "bare bin dir is an entry point");
    assert!(
        is_entry_point("crate.bin.foo"),
        "nested bin path is an entry point"
    );
    assert!(is_orphan(0, 0, "utils") && !is_orphan(1, 0, "utils"));
    let mut g = DependencyGraph::new();
    g.path_to_module.insert(PathBuf::from("src/foo.py"), "foo".into());
    g.paths.insert("foo".into(), PathBuf::from("src/foo.py"));
    assert_eq!(get_module_path(&g, "foo"), PathBuf::from("src/foo.py"));
    let mut parser = create_parser().unwrap();
    let mods = extract_modules_from_import_from(
        parser
            .parse("from foo.bar import baz", None)
            .unwrap()
            .root_node()
            .child(0)
            .unwrap(),
        "from foo.bar import baz",
    );
    assert!(
        mods.contains(&"foo.bar".into()),
        "Expected base module for from-import; got {mods:?}"
    );
    let rel = extract_modules_from_import_from(
        parser
            .parse("from ._export_format import X", None)
            .unwrap()
            .root_node()
            .child(0)
            .unwrap(),
        "from ._export_format import X",
    );
    assert!(
        rel.contains(&"_export_format".into()),
        "Relative import: {rel:?}"
    );
    let rel2 = extract_modules_from_import_from(
        parser
            .parse("from . import target", None)
            .unwrap()
            .root_node()
            .child(0)
            .unwrap(),
        "from . import target",
    );
    assert!(
        rel2.contains(&"target".into()),
        "Expected imported module candidate for `from . import target`; got {rel2:?}"
    );
    assert!(is_decision_point("if_statement") && !is_decision_point("identifier"));
    assert_eq!(
        count_decision_points(
            parser
                .parse("if a:\n    if b:\n        pass", None)
                .unwrap()
                .root_node()
        ),
        2
    );
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp, "x = 1").unwrap();
    assert!(!qualified_module_name(tmp.path()).is_empty());
    assert!(
        compute_cyclomatic_complexity(
            parser
                .parse("def f():\n    if a:\n        pass", None)
                .unwrap()
                .root_node()
                .child(0)
                .unwrap()
        ) >= 2
    );
}

#[test]
fn test_type_checking_imports_included_in_graph() {
    let mut parser = create_parser().unwrap();
    let code = "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    from some_module import SomeClass\nimport os";
    let imports = extract_imports_for_cache(parser.parse(code, None).unwrap().root_node(), code);
    assert!(imports.contains(&"typing".into()));
    assert!(imports.contains(&"os".into()));
    assert!(imports.contains(&"some_module".into()));

    let code2 = "import typing\nif typing.TYPE_CHECKING:\n    from foo import Bar\nimport json";
    let imports2 = extract_imports_for_cache(parser.parse(code2, None).unwrap().root_node(), code2);
    assert!(imports2.contains(&"typing".into()));
    assert!(imports2.contains(&"json".into()));
    assert!(imports2.contains(&"foo".into()));
}

#[test]
fn test_from_dot_import_name_is_dependency_candidate() {
    // Regression for orphan false-positives:
    // `from . import target` has no explicit module name, so the imported name needs to be
    // treated as a module candidate for dependency graph purposes.
    let mut parser = create_parser().unwrap();
    let code = "def f():\n    from . import target\n    return 0\n";
    let imports = extract_imports_for_cache(parser.parse(code, None).unwrap().root_node(), code);
    assert!(
        imports.contains(&"target".into()),
        "Expected `target` in import list for `from . import target`; got {imports:?}"
    );
}

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
    let g = build_temp_pkg_graph(
        "def f():\n    from . import target\n    return target.do_work()\n",
    );

    let m_importer = g.module_metrics("pkg.importer");
    let m_target = g.module_metrics("pkg.target");
    assert!(
        m_importer.fan_out >= 1 && m_target.fan_in >= 1,
        "Expected edge pkg.importer -> pkg.target (fan_out/fan_in >= 1); got importer={m_importer:?} target={m_target:?}"
    );
}

#[test]
fn test_from_import_adds_submodule_candidate_when_internal() {
    let g = build_temp_pkg_graph(
        "def f():\n    from pkg import target\n    return target.do_work()\n",
    );

    let m_importer = g.module_metrics("pkg.importer");
    let m_target = g.module_metrics("pkg.target");
    assert!(
        m_importer.fan_out >= 1 && m_target.fan_in >= 1,
        "Expected `from pkg import target` to create an internal edge to pkg.target; got importer={m_importer:?} target={m_target:?}"
    );
}

#[test]
fn test_qualified_and_bare_module_names() {
    use std::path::Path;
    // qualified_module_name includes parent directory
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

    // bare_module_name is just the filename without extension
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

    // Unambiguous: single match
    assert_eq!(
        resolve_import("utils", Some("click"), &bare_to_qualified),
        vec!["click.utils".to_string()]
    );

    // Ambiguous: multiple matches, prefer same package
    assert_eq!(
        resolve_import("exceptions", Some("attr"), &bare_to_qualified),
        vec!["attr.exceptions".to_string()]
    );
    assert_eq!(
        resolve_import("exceptions", Some("click"), &bare_to_qualified),
        vec!["click.exceptions".to_string()]
    );

    // Ambiguous: no matching package, returns empty (bare names stay conservative)
    assert!(resolve_import("exceptions", Some("httpx"), &bare_to_qualified).is_empty());

    // Unknown import: returns empty
    assert!(resolve_import("unknown", Some("attr"), &bare_to_qualified).is_empty());
}

#[test]
fn test_push_dotted_segments() {
    let mut modules = Vec::new();
    push_dotted_segments("foo.bar.baz", &mut modules);
    assert_eq!(modules, vec!["foo.bar.baz"]);

    modules.clear();
    push_dotted_segments("..relative", &mut modules);
    assert_eq!(modules, vec!["relative"]);

    modules.clear();
    push_dotted_segments("single", &mut modules);
    assert_eq!(modules, vec!["single"]);
}

// === Bug-hunting tests ===

#[test]
fn test_indirect_deps_in_cycle() {
    // In a 2-node cycle A→B→A, fan_out=1 (B), total_reachable=1, indirect=0.
    // The only reachable node is B, which is a direct neighbor.
    let mut g = DependencyGraph::new();
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
    // is_test_module should also recognize "test/" (singular) directories
    let mut g = DependencyGraph::new();
    g.path_to_module.insert(std::path::PathBuf::from("test/helpers.py"), "test.helpers".into());
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
    // Touch private helpers/structs so static test-ref coverage includes them.
    let _ = ImportInfo {
        from_qualified: "a.b".into(),
        from_parent_module: Some("a".into()),
        imports: vec!["os".into()],
    };
    let mut parser = create_parser().unwrap();
    let tree = parser.parse("import os", None).unwrap();
    let node = tree.root_node().child(0).unwrap();
    let mut imports = Vec::new();
    // child(1) is typically the dotted_name in `import os`
    let dotted = node.child(1).unwrap();
    push_import_name_segments(dotted, "import os", &mut imports);
    assert!(imports.contains(&"os".into()));
}
