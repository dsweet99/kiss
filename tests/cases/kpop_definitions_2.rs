use kiss::config::Config;
use kiss::graph::{analyze_graph, build_dependency_graph};
use kiss::parsing::{ParsedFile, create_parser, parse_file};
use std::path::Path;

fn parse_py(path: &Path) -> ParsedFile {
    let mut parser = create_parser().expect("parser should initialize");
    parse_file(&mut parser, path).expect("should parse fixture")
}

fn orphan_viols_for_temp_pkg(importer_code: &str) -> Vec<kiss::Violation> {
    use std::fs;
    use tempfile::TempDir;

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
    analyze_graph(&graph, &Config::python_defaults(), true)
}

#[test]
fn kpop_orphan_module_lazy_imports_try_to_break_it_in_10_ways() {
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
    let viols = analyze_graph(&graph, &Config::python_defaults(), true);

    assert!(
        !viols
            .iter()
            .any(|v| v.metric == "orphan_module" && v.unit_name == "a.pkg.target"),
        "Expected `a.pkg.target` not to be orphan (it is imported relatively). Got:\n{viols:#?}"
    );
}

#[test]
fn kpop_orphan_module_relative_import_two_dots_should_resolve_parent_package() {
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
    let viols = analyze_graph(&graph, &Config::python_defaults(), true);

    assert!(
        !viols
            .iter()
            .any(|v| v.metric == "orphan_module" && v.unit_name == "a.pkg.target"),
        "Expected `a.pkg.target` not to be orphan under `from .. import target`. Got:\n{viols:#?}"
    );
}

#[test]
fn kpop_orphan_module_dynamic_import_importlib_import_module_string_literal() {
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
    let viols = analyze_graph(&graph, &Config::python_defaults(), true);

    assert!(
        !viols
            .iter()
            .any(|v| v.metric == "orphan_module" && v.unit_name == "pkg.target"),
        "Expected `pkg.target` not to be orphan when dynamically imported via importlib.import_module(\"pkg.target\"). Got:\n{viols:#?}"
    );
}
