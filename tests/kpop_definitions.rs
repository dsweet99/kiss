use common::first_function_node;
use kiss::config::Config;
use kiss::graph::{analyze_graph, build_dependency_graph};
use kiss::parsing::{ParsedFile, create_parser, parse_file};
use kiss::py_metrics::compute_function_metrics;
use kiss::units::{CodeUnitKind, count_code_units, extract_code_units};
use kiss::discovery::{find_python_files, find_source_files_with_ignore};
use std::path::Path;
mod common;

fn parse_py(path: &Path) -> ParsedFile {
    let mut parser = create_parser().expect("parser should initialize");
    parse_file(&mut parser, path).expect("should parse fixture")
}

fn orphan_viols_for_temp_pkg(importer_code: &str) -> Vec<kiss::Violation> {
    use std::fs;
    use tempfile::TempDir;

    // Important: orphan_module is skipped for paths containing a `tests/` component.
    // So we generate fixtures in a temp directory with a `src/` root.
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    let pkg = src.join("pkg");
    fs::create_dir_all(&pkg).unwrap();

    fs::write(pkg.join("__init__.py"), "").unwrap();
    fs::write(pkg.join("target.py"), "def do_work():\n    return 42\n").unwrap();
    fs::write(pkg.join("importer.py"), importer_code).unwrap();

    let importer = parse_py(&pkg.join("importer.py"));
    let target = parse_py(&pkg.join("target.py"));
    let init = parse_py(&pkg.join("__init__.py"));

    let parsed_files: Vec<&ParsedFile> = vec![&importer, &target, &init];
    let graph = build_dependency_graph(&parsed_files);
    analyze_graph(&graph, &Config::python_defaults())
}

fn h1_module_code_unit_exists() {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp, "").unwrap();
    let p = parse_py(tmp.path());
    let units = extract_code_units(&p);
    assert!(units.iter().any(|u| u.kind == CodeUnitKind::Module));
}

fn h2_async_function_is_code_unit() {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp, "async def af():\n    return 1\n").unwrap();
    let p = parse_py(tmp.path());
    let units = extract_code_units(&p);
    assert!(units.iter().any(|u| u.kind == CodeUnitKind::Function && u.name == "af"));
}

fn h3_decorated_function_is_code_unit() {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp, "@dec\ndef df():\n    return 1\n").unwrap();
    let p = parse_py(tmp.path());
    let units = extract_code_units(&p);
    assert!(units.iter().any(|u| u.kind == CodeUnitKind::Function && u.name == "df"));
}

fn h4_class_is_code_unit() {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp, "class C:\n    pass\n").unwrap();
    let p = parse_py(tmp.path());
    let units = extract_code_units(&p);
    assert!(units.iter().any(|u| u.kind == CodeUnitKind::Class && u.name == "C"));
}

fn h5_method_is_code_unit() {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp, "class C:\n    def m(self):\n        return 1\n").unwrap();
    let p = parse_py(tmp.path());
    let units = extract_code_units(&p);
    assert!(units.iter().any(|u| u.kind == CodeUnitKind::Method && u.name == "m"));
}

fn h6_nested_function_is_code_unit() {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(
        tmp,
        "def outer():\n    def inner():\n        return 1\n    return 2\n"
    )
    .unwrap();
    let p = parse_py(tmp.path());
    let units = extract_code_units(&p);
    assert!(units.iter().any(|u| u.kind == CodeUnitKind::Function && u.name == "inner"));
}

fn h7_nested_class_is_code_unit() {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp, "class A:\n    class B:\n        pass\n").unwrap();
    let p = parse_py(tmp.path());
    let units = extract_code_units(&p);
    assert!(units.iter().any(|u| u.kind == CodeUnitKind::Class && u.name == "B"));
}

fn h8_decorated_method_is_code_unit() -> ParsedFile {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp, "class C:\n    @dec\n    def m(self):\n        return 1\n").unwrap();
    parse_py(tmp.path())
}

fn h9_count_matches_extraction(p: &ParsedFile) {
    assert_eq!(count_code_units(p), extract_code_units(p).len());
}

fn h10_code_unit_has_byte_range(p: &ParsedFile) {
    let units = extract_code_units(p);
    let f = units.iter().find(|u| u.name == "m").unwrap();
    assert!(f.start_byte < f.end_byte);
}

fn file_h1_to_h3_discovery(tmp: &tempfile::TempDir) {
    use std::fs;
    // H1: .py files are discoverable
    fs::write(tmp.path().join("a.py"), "x=1\n").unwrap();
    // H2: non-.py files are ignored by find_python_files
    fs::write(tmp.path().join("b.txt"), "x\n").unwrap();
    // H3: nested .py files are discoverable
    let sub = tmp.path().join("sub");
    fs::create_dir(&sub).unwrap();
    fs::write(sub.join("c.py"), "x=1\n").unwrap();
    let py = find_python_files(tmp.path());
    assert!(py.iter().any(|p| p.ends_with("a.py")));
    assert!(py.iter().any(|p| p.ends_with("c.py")));
    assert!(!py.iter().any(|p| p.ends_with("b.txt")));
}

fn file_h4_ignore_prefixes(tmp: &tempfile::TempDir) {
    use std::fs;
    let ignored_dir = tmp.path().join("fake_data");
    fs::create_dir(&ignored_dir).unwrap();
    fs::write(ignored_dir.join("d.py"), "x=1\n").unwrap();
    let sources = find_source_files_with_ignore(tmp.path(), &[String::from("fake_")]);
    assert!(!sources.iter().any(|sf| sf.path.ends_with("d.py")));
}

fn file_h5_pycache_is_ignored(tmp: &tempfile::TempDir) {
    use std::fs;
    let pycache = tmp.path().join("__pycache__");
    fs::create_dir(&pycache).unwrap();
    fs::write(pycache.join("e.py"), "x=1\n").unwrap();
    let sources = find_source_files_with_ignore(tmp.path(), &[]);
    assert!(!sources.iter().any(|sf| sf.path.ends_with("e.py")));
}

fn file_h6_kissignore_excludes(tmp: &tempfile::TempDir) {
    use std::fs;
    let ignored = tmp.path().join("ignored");
    fs::create_dir(&ignored).unwrap();
    fs::write(ignored.join("f.py"), "x=1\n").unwrap();
    fs::write(tmp.path().join(".kissignore"), "ignored/\n").unwrap();
    let sources = find_source_files_with_ignore(tmp.path(), &[]);
    assert!(!sources.iter().any(|sf| sf.path.ends_with("f.py")));
}

fn file_h7_rust_files_discovered(tmp: &tempfile::TempDir) {
    use std::fs;
    fs::write(tmp.path().join("g.rs"), "fn main() {}\n").unwrap();
    let sources = find_source_files_with_ignore(tmp.path(), &[]);
    assert!(sources.iter().any(|sf| sf.path.ends_with("g.rs")));
}

fn file_h8_missing_dir_is_empty() {
    assert!(find_python_files(Path::new("nonexistent_dir_for_kpop")).is_empty());
}

fn file_h9_and_h10_parsing(tmp: &tempfile::TempDir) {
    let mut parser = create_parser().unwrap();
    let parsed = parse_file(&mut parser, &tmp.path().join("a.py")).unwrap();
    assert!(parsed.path.ends_with("a.py"));
    assert!(!parsed.tree.root_node().has_error());
}

#[test]
fn bug_lazy_import_should_create_graph_edge_and_prevent_orphan() {
    // DEFINITION: [graph_edge] A dependency between two modules.
    //
    // Hypothesis: When module A imports module B inside a function body (lazy import),
    // the dependency graph misses this edge. Module B then appears to have fan_in=0
    // and fan_out=0, so it is incorrectly flagged as orphan.
    //
    // Prediction: `lazy_target` should have fan_in >= 1 (imported by `lazy_importer`)
    // and should NOT be flagged as orphan_module.
    //
    // Test: Build a graph from two fixture files where the import is inside a function body.
    let importer = parse_py(Path::new("tests/fake_python/lazy_importer.py"));
    let target = parse_py(Path::new("tests/fake_python/lazy_target.py"));

    let parsed_files: Vec<&ParsedFile> = vec![&importer, &target];
    let graph = build_dependency_graph(&parsed_files);
    let viols = analyze_graph(&graph, &Config::python_defaults());

    let orphan_viols: Vec<_> = viols.iter().filter(|v| v.metric == "orphan_module").collect();
    assert!(
        orphan_viols.is_empty(),
        "lazy_target should not be orphan when imported inside a function; got:\n{orphan_viols:#?}"
    );
}

#[test]
fn bug_graph_edge_dotted_import_should_create_internal_edge() {
    // DEFINITION: [graph_edge] A dependency between two modules.
    //
    // Hypothesis: dotted imports like `import pkg1.submod` fail to resolve to an internal module
    // when the analyzed module names are qualified with additional path prefixes
    // (e.g. "tests.fake_python.pkg1.submod"). That would create an external node "pkg1.submod"
    // and miss the internal edge, making internal modules appear orphan.
    //
    // Prediction: when analyzing `tests/fake_python/imports_pkg1_submod.py`, the dependency edge
    // should target the internal module `tests.fake_python.pkg1.submod`, and that module should
    // not be reported as orphan.
    //
    // Test: Build a graph from the fixture set and assert no orphan for pkg1/pkg2 submods.
    let pkg1_sub = parse_py(Path::new("tests/fake_python/pkg1/submod.py"));
    let pkg1_init = parse_py(Path::new("tests/fake_python/pkg1/__init__.py"));
    let pkg2_sub = parse_py(Path::new("tests/fake_python/pkg2/submod.py"));
    let pkg2_init = parse_py(Path::new("tests/fake_python/pkg2/__init__.py"));
    let importer1 = parse_py(Path::new("tests/fake_python/imports_pkg1_submod.py"));
    let importer2 = parse_py(Path::new("tests/fake_python/imports_pkg2_submod.py"));

    let parsed_files: Vec<&ParsedFile> = vec![
        &pkg1_sub,
        &pkg1_init,
        &pkg2_sub,
        &pkg2_init,
        &importer1,
        &importer2,
    ];
    let graph = build_dependency_graph(&parsed_files);
    let viols = analyze_graph(&graph, &Config::python_defaults());

    // Expected (correct): no orphan violations for these modules since they are imported.
    assert!(
        !viols.iter().any(|v| v.metric == "orphan_module"),
        "expected no orphan_module violations; got:\n{viols:#?}"
    );
}

#[test]
fn bug_statement_definition_should_exclude_nested_function_bodies() {
    // DEFINITION: [statement] A statement inside a function/method body (not an import or a class/function signature).
    //
    // Hypothesis: statement counting for a function includes statements inside nested function bodies,
    // which violates the definition (those statements are not in the outer function body).
    //
    // Prediction: In this fixture, the outer function has exactly 1 statement in its body: `return 1`.
    // The nested function body statement (`x = 1`) should not be counted toward the outer function.
    //
    // Test: compute metrics for outer function and assert statements == 1.
    let p = {
        use std::io::Write;
        let code = "def outer():\n    def inner():\n        x = 1\n        return x\n    return 1\n";
        let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
        write!(tmp, "{code}").unwrap();
        parse_py(tmp.path())
    };

    let outer = first_function_node(&p);
    let m = compute_function_metrics(outer, &p.source);
    assert_eq!(
        m.statements, 1,
        "expected only the outer return statement to count"
    );
}

#[test]
fn bug_graph_node_definition_should_exclude_external_imports() {
    // DEFINITION: [graph_node] A module (file) in the dependency graph.
    //
    // Hypothesis: The dependency graph includes nodes for external imports (like stdlib modules),
    // even though they are not files in the analyzed codebase, inflating graph_nodes.
    //
    // Prediction: For two analyzed files `a.py` and `b.py`, graph_nodes should be 2 even if `a.py`
    // imports external modules.
    //
    // Test: Build a graph from two real fixture files plus external imports, and assert node count
    // equals internal-file count.
    let a = {
        use std::io::Write;
        let code = "import os\nimport json\nimport b\n";
        let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
        write!(tmp, "{code}").unwrap();
        parse_py(tmp.path())
    };
    // Name b.py explicitly so it's a separate internal file.
    let b = {
        use std::io::Write;
        let code = "x = 1\n";
        let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
        write!(tmp, "{code}").unwrap();
        parse_py(tmp.path())
    };

    let parsed_files: Vec<&ParsedFile> = vec![&a, &b];
    let graph = build_dependency_graph(&parsed_files);

    let internal_count = graph.paths.len();
    let node_count = graph.graph.node_count();
    assert_eq!(
        node_count, internal_count,
        "expected graph_nodes to count only analyzed files (internal modules)"
    );
}

#[test]
fn kpop_code_unit_definition_no_bug_found_in_10_hypotheses() {
    // DEFINITION: [code_unit]
    //
    // KPOP (10 hypotheses). Each assertion corresponds to a falsifying test.
    // If all pass, we record BUG: None for this definition.
    h1_module_code_unit_exists();
    h2_async_function_is_code_unit();
    h3_decorated_function_is_code_unit();
    h4_class_is_code_unit();
    h5_method_is_code_unit();
    h6_nested_function_is_code_unit();
    h7_nested_class_is_code_unit();
    let p = h8_decorated_method_is_code_unit();
    h9_count_matches_extraction(&p);
    h10_code_unit_has_byte_range(&p);
}

#[test]
fn kpop_file_definition_no_bug_found_in_10_hypotheses() {
    // DEFINITION: [file]
    //
    // KPOP (10 hypotheses) against discovery/parsing expectations for what counts as an analyzed file.
    // If all pass, we record BUG: None for this definition.
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();

    file_h1_to_h3_discovery(&tmp);
    file_h4_ignore_prefixes(&tmp);
    file_h5_pycache_is_ignored(&tmp);
    file_h6_kissignore_excludes(&tmp);
    file_h7_rust_files_discovered(&tmp);
    file_h8_missing_dir_is_empty();
    file_h9_and_h10_parsing(&tmp);
}

#[test]
fn kpop_orphan_module_lazy_imports_try_to_break_it_in_10_ways() {
    // DEFINITION: [orphan_module] A non-test module with fan_in=0 and fan_out=0 (excluding entry points).
    //
    // Restated problem: `kiss check` sometimes flags Python modules as orphan even though they *are*
    // imported, especially when imports are "lazy" (inside function bodies).
    //
    // We'll run up to 10 falsifying attempts (KPOP-style). We stop as soon as we find a repro that
    // produces an orphan violation for `pkg.target` even though the importer code imports it.

    // KPOP: 10 attempts. Each case should NOT produce an orphan for `pkg.target`.
    let cases: [(&str, &str); 10] = [
        (
            "attempt 1: from . import target",
            "def f():\n    from . import target\n    return target.do_work()\n",
        ),
        (
            "attempt 2: import pkg.target",
            "def f():\n    import pkg.target\n    return pkg.target.do_work()\n",
        ),
        (
            "attempt 3: from pkg import target",
            "def f():\n    from pkg import target\n    return target.do_work()\n",
        ),
        (
            "attempt 4: from pkg.target import do_work",
            "def f():\n    from pkg.target import do_work\n    return do_work()\n",
        ),
        (
            "attempt 5: try/except from pkg import target",
            "def f():\n    try:\n        from pkg import target\n    except ImportError:\n        return 0\n    return target.do_work()\n",
        ),
        (
            "attempt 6: conditional from pkg import target",
            "def f(flag: bool):\n    if flag:\n        from pkg import target\n        return target.do_work()\n    return 0\n",
        ),
        (
            "attempt 7: import pkg.target as t",
            "def f():\n    import pkg.target as t\n    return t.do_work()\n",
        ),
        (
            "attempt 8: parenthesized from pkg.target import (do_work)",
            "def f():\n    from pkg.target import (do_work)\n    return do_work()\n",
        ),
        (
            "attempt 9: from pkg import target as t",
            "def f():\n    from pkg import target as t\n    return t.do_work()\n",
        ),
        (
            "attempt 10: from .target import do_work",
            "def f():\n    from .target import do_work\n    return do_work()\n",
        ),
    ];

    for (name, code) in cases {
        let viols = orphan_viols_for_temp_pkg(code);
        assert!(
            !viols
                .iter()
                .any(|v| v.metric == "orphan_module" && v.unit_name == "pkg.target"),
            "REPRO FOUND ({name}): got orphan_module for pkg.target:\n{viols:#?}"
        );
    }
}

#[test]
fn kpop_orphan_module_ambiguous_bare_name_relative_import_should_resolve_to_same_package() {
    // Restated problem: orphan detection can false-positive if the dependency graph misses edges.
    //
    // Hypothesis: If two internal modules share the same bare name (e.g., `a.pkg.target` and
    // `b.pkg.target`), a relative import `from . import target` inside `a.pkg.importer` will not
    // resolve to `a.pkg.target` because resolution only uses the leaf directory name ("pkg") and
    // cannot disambiguate. The edge is dropped, so `a.pkg.target` is incorrectly flagged orphan.
    //
    // Prediction: With two `target.py` modules, `a.pkg.target` should still NOT be orphan.
    //
    // Test: Create a temp `src/` tree with both packages and analyze the graph.
    use std::fs;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");

    for root in ["a", "b"] {
        let pkg = src.join(root).join("pkg");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("__init__.py"), "").unwrap();
        fs::write(pkg.join("target.py"), "def do_work():\n    return 42\n").unwrap();
    }
    // Importer only in a/
    let a_pkg = src.join("a").join("pkg");
    fs::write(
        a_pkg.join("importer.py"),
        "def f():\n    from . import target\n    return target.do_work()\n",
    )
    .unwrap();

    let a_importer = parse_py(&a_pkg.join("importer.py"));
    let a_target = parse_py(&a_pkg.join("target.py"));
    let a_init = parse_py(&a_pkg.join("__init__.py"));
    let b_target = parse_py(&src.join("b").join("pkg").join("target.py"));
    let b_init = parse_py(&src.join("b").join("pkg").join("__init__.py"));

    let parsed_files: Vec<&ParsedFile> = vec![&a_importer, &a_target, &a_init, &b_target, &b_init];
    let graph = build_dependency_graph(&parsed_files);
    let viols = analyze_graph(&graph, &Config::python_defaults());

    assert!(
        !viols
            .iter()
            .any(|v| v.metric == "orphan_module" && v.unit_name == "a.pkg.target"),
        "Expected `a.pkg.target` not to be orphan (it is imported relatively). Got:\n{viols:#?}"
    );
}

#[test]
fn kpop_orphan_module_relative_import_two_dots_should_resolve_parent_package() {
    // Restated problem: orphan detection false-positives when relative imports don't create edges.
    //
    // Hypothesis: `from .. import target` is treated like importing bare `target`, and resolution
    // doesn't walk up parent packages. So the edge to `a.pkg.target` is missed and it's flagged orphan.
    //
    // Prediction: In a package `a.pkg.sub`, `from .. import target` should prevent `a.pkg.target`
    // from being orphan.
    use std::fs;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");

    let a_pkg = src.join("a").join("pkg");
    let a_sub = a_pkg.join("sub");
    fs::create_dir_all(&a_sub).unwrap();

    fs::write(a_pkg.join("__init__.py"), "").unwrap();
    fs::write(a_sub.join("__init__.py"), "").unwrap();
    fs::write(a_pkg.join("target.py"), "def do_work():\n    return 42\n").unwrap();
    fs::write(
        a_sub.join("importer.py"),
        "def f():\n    from .. import target\n    return target.do_work()\n",
    )
    .unwrap();

    let importer = parse_py(&a_sub.join("importer.py"));
    let sub_init = parse_py(&a_sub.join("__init__.py"));
    let pkg_init = parse_py(&a_pkg.join("__init__.py"));
    let target = parse_py(&a_pkg.join("target.py"));

    let parsed_files: Vec<&ParsedFile> = vec![&importer, &sub_init, &pkg_init, &target];
    let graph = build_dependency_graph(&parsed_files);
    let viols = analyze_graph(&graph, &Config::python_defaults());

    assert!(
        !viols
            .iter()
            .any(|v| v.metric == "orphan_module" && v.unit_name == "a.pkg.target"),
        "Expected `a.pkg.target` not to be orphan under `from .. import target`. Got:\n{viols:#?}"
    );
}

#[test]
fn kpop_orphan_module_dynamic_import_importlib_import_module_string_literal() {
    // Restated problem: orphan detection can be wrong if dynamic imports aren't treated as dependencies.
    //
    // Hypothesis: `importlib.import_module("pkg.target")` is not recognized as an import edge,
    // so `pkg.target` is incorrectly flagged orphan.
    //
    // Prediction: With a string-literal dynamic import, `pkg.target` should NOT be orphan.
    use std::fs;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    let pkg = src.join("pkg");
    fs::create_dir_all(&pkg).unwrap();

    fs::write(pkg.join("__init__.py"), "").unwrap();
    fs::write(pkg.join("target.py"), "def do_work():\n    return 42\n").unwrap();
    fs::write(
        pkg.join("importer.py"),
        "def f():\n    import importlib\n    m = importlib.import_module(\"pkg.target\")\n    return m.do_work()\n",
    )
    .unwrap();

    let importer = parse_py(&pkg.join("importer.py"));
    let target = parse_py(&pkg.join("target.py"));
    let init = parse_py(&pkg.join("__init__.py"));

    let parsed_files: Vec<&ParsedFile> = vec![&importer, &target, &init];
    let graph = build_dependency_graph(&parsed_files);
    let viols = analyze_graph(&graph, &Config::python_defaults());

    assert!(
        !viols
            .iter()
            .any(|v| v.metric == "orphan_module" && v.unit_name == "pkg.target"),
        "Expected `pkg.target` not to be orphan when dynamically imported via importlib.import_module(\"pkg.target\"). Got:\n{viols:#?}"
    );
}

