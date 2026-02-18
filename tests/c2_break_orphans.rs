use kiss::config::Config;
use kiss::graph::{analyze_graph, build_dependency_graph};
use kiss::parsing::{ParsedFile, create_parser, parse_file};
use std::path::Path;

fn parse_py(path: &Path) -> ParsedFile {
    let mut parser = create_parser().expect("parser should initialize");
    parse_file(&mut parser, path).expect("should parse fixture")
}

#[test]
fn c2_break_1_empty_init_py_is_falsely_orphaned() {
    // C2 Break #1: __init__.py entry point check is dead code.
    //
    // qualified_module_name renames pkg/__init__.py → "pkg". Then is_entry_point("pkg")
    // extracts bare name "pkg", which never matches "__init__". The __init__ exemption
    // in is_entry_point is unreachable for __init__.py inside a package directory.
    //
    // Prediction: empty __init__.py should NOT be flagged as orphan_module.
    use std::fs;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    let pkg = src.join("pkg");
    fs::create_dir_all(&pkg).unwrap();

    fs::write(pkg.join("__init__.py"), "").unwrap();
    fs::write(pkg.join("mod_a.py"), "from pkg.mod_b import helper\n").unwrap();
    fs::write(pkg.join("mod_b.py"), "def helper():\n    return 42\n").unwrap();

    let init = parse_py(&pkg.join("__init__.py"));
    let mod_a = parse_py(&pkg.join("mod_a.py"));
    let mod_b = parse_py(&pkg.join("mod_b.py"));

    let parsed_files: Vec<&ParsedFile> = vec![&init, &mod_a, &mod_b];
    let graph = build_dependency_graph(&parsed_files);
    let viols = analyze_graph(&graph, &Config::python_defaults());

    assert!(
        !viols
            .iter()
            .any(|v| v.metric == "orphan_module" && v.unit_name == "pkg"),
        "Empty __init__.py (qualified as 'pkg') should not be orphan — it is a package marker. Got:\n{viols:#?}"
    );
}

#[test]
fn c2_break_2_ambiguous_dotted_suffix_skips_parent_fallback() {
    // C2 Break #2: resolve_import's dotted branch returns None when matches.len() > 1,
    // even though the parent-module fallback could disambiguate.
    //
    // Both qualified names end with "services.handler" → matches.len() == 2.
    // The parent fallback (prefix "team_a.services.") would resolve to the correct one,
    // but is only tried when matches.is_empty(), not when ambiguous.
    //
    // Prediction: team_a.services.handler should NOT be orphan.
    use std::fs;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    let svc_a = src.join("team_a").join("services");
    let svc_b = src.join("team_b").join("services");
    fs::create_dir_all(&svc_a).unwrap();
    fs::create_dir_all(&svc_b).unwrap();

    fs::write(svc_a.join("__init__.py"), "").unwrap();
    fs::write(svc_a.join("handler.py"), "class Handler:\n    pass\n").unwrap();
    fs::write(
        svc_a.join("consumer.py"),
        "from services.handler import Handler\n",
    )
    .unwrap();
    fs::write(svc_b.join("__init__.py"), "").unwrap();
    fs::write(svc_b.join("handler.py"), "class OtherHandler:\n    pass\n").unwrap();

    let a_handler = parse_py(&svc_a.join("handler.py"));
    let a_consumer = parse_py(&svc_a.join("consumer.py"));
    let a_init = parse_py(&svc_a.join("__init__.py"));
    let b_handler = parse_py(&svc_b.join("handler.py"));
    let b_init = parse_py(&svc_b.join("__init__.py"));

    let parsed_files: Vec<&ParsedFile> =
        vec![&a_handler, &a_consumer, &a_init, &b_handler, &b_init];
    let graph = build_dependency_graph(&parsed_files);
    let viols = analyze_graph(&graph, &Config::python_defaults());

    assert!(
        !viols
            .iter()
            .any(|v| v.metric == "orphan_module" && v.unit_name == "team_a.services.handler"),
        "team_a.services.handler should not be orphan — consumer in same package imports it \
         via 'from services.handler import Handler'. Got:\n{viols:#?}"
    );
}

fn build_cross_pkg_registry_fixture() -> (tempfile::TempDir, Vec<std::path::PathBuf>) {
    use std::fs;
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");

    let app_core = src.join("app").join("core");
    let ext_core = src.join("ext").join("core");
    fs::create_dir_all(&app_core).unwrap();
    fs::create_dir_all(&ext_core).unwrap();
    for dir in ["api", "cli", "worker"] {
        fs::create_dir_all(src.join(dir)).unwrap();
    }

    fs::write(app_core.join("__init__.py"), "").unwrap();
    fs::write(app_core.join("registry.py"), "class Registry:\n    pass\n").unwrap();
    fs::write(ext_core.join("__init__.py"), "").unwrap();
    fs::write(ext_core.join("registry.py"), "class ExtRegistry:\n    pass\n").unwrap();

    let import_code = "def f():\n    from core.registry import Registry\n    return Registry()\n";
    fs::write(src.join("api").join("endpoint.py"), import_code).unwrap();
    fs::write(src.join("cli").join("command.py"), import_code).unwrap();
    fs::write(src.join("worker").join("task.py"), import_code).unwrap();

    let paths = vec![
        app_core.join("registry.py"),
        app_core.join("__init__.py"),
        ext_core.join("registry.py"),
        ext_core.join("__init__.py"),
        src.join("api").join("endpoint.py"),
        src.join("cli").join("command.py"),
        src.join("worker").join("task.py"),
    ];
    (tmp, paths)
}

#[test]
fn c2_break_3_many_cross_package_importers_all_fail_on_ambiguous_suffix() {
    // C2 Break #3: Repeated directory structures cause suffix ambiguity that makes ALL
    // importers — even from different packages — fail resolution simultaneously.
    // Both qualified names end with "core.registry" → matches.len() == 2 for every importer.
    //
    // Prediction: at least one registry module should NOT be orphan.
    let (_tmp, paths) = build_cross_pkg_registry_fixture();
    let parsed: Vec<_> = paths.iter().map(|p| parse_py(p)).collect();
    let refs: Vec<&ParsedFile> = parsed.iter().collect();
    let graph = build_dependency_graph(&refs);
    let viols = analyze_graph(&graph, &Config::python_defaults());

    let orphan_registries: Vec<_> = viols
        .iter()
        .filter(|v| v.metric == "orphan_module" && v.unit_name.contains("registry"))
        .collect();
    assert!(
        orphan_registries.is_empty(),
        "registry modules imported by 3 consumers should not all be orphan. Got:\n{orphan_registries:#?}"
    );
}

#[test]
fn c2_break_4_type_checking_only_import_makes_module_invisible() {
    // C2 Break #4: Imports inside `if TYPE_CHECKING:` blocks are skipped by extraction.
    // A module imported ONLY inside TYPE_CHECKING has zero fan_in in the graph, even though
    // it is genuinely used for type annotations.
    //
    // Prediction: a type-definitions module imported for annotations should NOT be orphan.
    use std::fs;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    let pkg = src.join("pkg");
    fs::create_dir_all(&pkg).unwrap();

    fs::write(pkg.join("__init__.py"), "").unwrap();
    fs::write(
        pkg.join("types.py"),
        "from dataclasses import dataclass\n\n\
         @dataclass\n\
         class UserInfo:\n\
         \x20\x20\x20\x20name: str\n\
         \x20\x20\x20\x20age: int\n",
    )
    .unwrap();
    fs::write(
        pkg.join("service.py"),
        "from __future__ import annotations\n\
         from typing import TYPE_CHECKING\n\n\
         if TYPE_CHECKING:\n\
         \x20\x20\x20\x20from pkg.types import UserInfo\n\n\
         def process(user: UserInfo) -> str:\n\
         \x20\x20\x20\x20return user.name\n",
    )
    .unwrap();
    fs::write(src.join("main.py"), "from pkg.service import process\n").unwrap();

    let types = parse_py(&pkg.join("types.py"));
    let service = parse_py(&pkg.join("service.py"));
    let init = parse_py(&pkg.join("__init__.py"));
    let main = parse_py(&src.join("main.py"));

    let parsed_files: Vec<&ParsedFile> = vec![&types, &service, &init, &main];
    let graph = build_dependency_graph(&parsed_files);
    let viols = analyze_graph(&graph, &Config::python_defaults());

    assert!(
        !viols
            .iter()
            .any(|v| v.metric == "orphan_module" && v.unit_name == "pkg.types"),
        "pkg.types should not be orphan — it is imported for type checking. Got:\n{viols:#?}"
    );
}

#[test]
fn c2_break_5_absolute_path_truncation_creates_qualified_name_collision() {
    // C2 Break #5: For absolute paths without a `src/` root, qualified_module_name truncates
    // to the last 2 directory components. Two files at different locations but with the same
    // last-2 dirs produce identical qualified names, silently merging into one graph node.
    //
    // Prediction: two files at different paths should produce distinct qualified names
    // (graph.paths should have 2 entries, not 1).
    use std::fs;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let first = tmp.path().join("aaa").join("shared").join("pkg");
    let second = tmp.path().join("bbb").join("shared").join("pkg");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();

    fs::write(first.join("target.py"), "class Foo:\n    pass\n").unwrap();
    fs::write(second.join("target.py"), "class Bar:\n    pass\n").unwrap();

    let first_target = parse_py(&first.join("target.py"));
    let second_target = parse_py(&second.join("target.py"));

    let parsed_files: Vec<&ParsedFile> = vec![&first_target, &second_target];
    let graph = build_dependency_graph(&parsed_files);

    assert_eq!(
        graph.paths.len(),
        2,
        "Two files at different absolute paths should produce distinct qualified names. \
         graph.paths has {} entries: {:?}",
        graph.paths.len(),
        graph.paths
    );
}
