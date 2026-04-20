use crate::common::first_function_node;
use kiss::config::Config;
use kiss::discovery::{find_python_files, find_source_files_with_ignore};
use kiss::graph::{analyze_graph, build_dependency_graph};
use kiss::parsing::{ParsedFile, create_parser, parse_file};
use kiss::py_metrics::compute_function_metrics;
use kiss::units::{CodeUnitKind, count_code_units, extract_code_units};
use std::path::Path;

fn parse_py(path: &Path) -> ParsedFile {
    let mut parser = create_parser().expect("parser should initialize");
    parse_file(&mut parser, path).expect("should parse fixture")
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
    assert!(
        units
            .iter()
            .any(|u| u.kind == CodeUnitKind::Function && u.name == "af")
    );
}

fn h3_decorated_function_is_code_unit() {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp, "@dec\ndef df():\n    return 1\n").unwrap();
    let p = parse_py(tmp.path());
    let units = extract_code_units(&p);
    assert!(
        units
            .iter()
            .any(|u| u.kind == CodeUnitKind::Function && u.name == "df")
    );
}

fn h4_class_is_code_unit() {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp, "class C:\n    pass\n").unwrap();
    let p = parse_py(tmp.path());
    let units = extract_code_units(&p);
    assert!(
        units
            .iter()
            .any(|u| u.kind == CodeUnitKind::Class && u.name == "C")
    );
}

fn h5_method_is_code_unit() {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp, "class C:\n    def m(self):\n        return 1\n").unwrap();
    let p = parse_py(tmp.path());
    let units = extract_code_units(&p);
    assert!(
        units
            .iter()
            .any(|u| u.kind == CodeUnitKind::Method && u.name == "m")
    );
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
    assert!(
        units
            .iter()
            .any(|u| u.kind == CodeUnitKind::Function && u.name == "inner")
    );
}

fn h7_nested_class_is_code_unit() {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(tmp, "class A:\n    class B:\n        pass\n").unwrap();
    let p = parse_py(tmp.path());
    let units = extract_code_units(&p);
    assert!(
        units
            .iter()
            .any(|u| u.kind == CodeUnitKind::Class && u.name == "B")
    );
}

fn h8_decorated_method_is_code_unit() -> ParsedFile {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    write!(
        tmp,
        "class C:\n    @dec\n    def m(self):\n        return 1\n"
    )
    .unwrap();
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
    let importer = parse_py(Path::new("tests/fake_python/lazy_importer.py"));
    let target = parse_py(Path::new("tests/fake_python/lazy_target.py"));

    let parsed_files: Vec<&ParsedFile> = vec![&importer, &target];
    let graph = build_dependency_graph(&parsed_files);
    let viols = analyze_graph(&graph, &Config::python_defaults(), true);

    let orphan_viols: Vec<_> = viols
        .iter()
        .filter(|v| v.metric == "orphan_module")
        .collect();
    assert!(
        orphan_viols.is_empty(),
        "lazy_target should not be orphan when imported inside a function; got:\n{orphan_viols:#?}"
    );
}

#[test]
fn bug_graph_edge_dotted_import_should_create_internal_edge() {
    let pkg1_sub = parse_py(Path::new("tests/fake_python/pkg1/submod.py"));
    let pkg1_init = parse_py(Path::new("tests/fake_python/pkg1/__init__.py"));
    let pkg2_sub = parse_py(Path::new("tests/fake_python/pkg2/submod.py"));
    let pkg2_init = parse_py(Path::new("tests/fake_python/pkg2/__init__.py"));
    let importer1 = parse_py(Path::new("tests/fake_python/imports_pkg1_submod.py"));
    let importer2 = parse_py(Path::new("tests/fake_python/imports_pkg2_submod.py"));

    let parsed_files: Vec<&ParsedFile> = vec![
        &pkg1_sub, &pkg1_init, &pkg2_sub, &pkg2_init, &importer1, &importer2,
    ];
    let graph = build_dependency_graph(&parsed_files);
    let viols = analyze_graph(&graph, &Config::python_defaults(), true);

    assert!(
        !viols.iter().any(|v| v.metric == "orphan_module"),
        "expected no orphan_module violations; got:\n{viols:#?}"
    );
}

#[test]
fn bug_statement_definition_should_exclude_nested_function_bodies() {
    let p = {
        use std::io::Write;
        let code =
            "def outer():\n    def inner():\n        x = 1\n        return x\n    return 1\n";
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
    let a = {
        use std::io::Write;
        let code = "import os\nimport json\nimport b\n";
        let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
        write!(tmp, "{code}").unwrap();
        parse_py(tmp.path())
    };
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
