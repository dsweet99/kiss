use kiss::rust_graph::build_rust_dependency_graph;
use kiss::rust_parsing::{ParsedRustFile, parse_rust_file};
use kiss::graph::analyze_graph;
use kiss::config::Config;
use std::path::Path;

fn parse_rs(path: &Path) -> ParsedRustFile {
    parse_rust_file(path).expect("parse rust fixture")
}

#[test]
fn bug_rust_transitive_dependencies_should_not_count_external_imports() {
    // RULE: [Rust] [transitive_dependencies]
    //
    // Hypothesis: transitive dependency counts include external crates/stdlib paths, inflating coupling.
    // Prediction: rust_graph_ext_a has exactly 1 internal transitive dependency (rust_graph_ext_b).
    let a = parse_rs(Path::new("tests/fake_rust/rust_graph_ext_a.rs"));
    let b = parse_rs(Path::new("tests/fake_rust/rust_graph_ext_b.rs"));
    let parsed: Vec<&ParsedRustFile> = vec![&a, &b];
    let g = build_rust_dependency_graph(&parsed);

    let m = g.module_metrics("fake_rust.rust_graph_ext_a");
    assert_eq!(m.transitive_dependencies, 1);
}

#[test]
fn bug_orphan_module_should_not_flag_crate_use_imports_in_rust() {
    // RULE: [Rust] [orphan_module]
    //
    // Hypothesis: Rust `use crate::foo::...` imports are ignored, so internal modules
    // only referenced via `crate::` appear orphan.
    //
    // Prediction: When `orphan_crate_use_importer.rs` uses `crate::orphan_crate_use_target`,
    // the target module should not be flagged as orphan_module.
    //
    // Test: Copy fixtures into a temp `src/` crate root, build graph, and run orphan analysis.
    use std::fs;
    use tempfile::TempDir;

    let importer_fixture = fs::read_to_string("tests/fake_rust/orphan_crate_use_importer.rs").unwrap();
    let target_fixture = fs::read_to_string("tests/fake_rust/orphan_crate_use_target.rs").unwrap();

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();

    // Declare the modules so Rust semantics match the fixture.
    fs::write(
        src.join("lib.rs"),
        "mod orphan_crate_use_importer;\nmod orphan_crate_use_target;\n",
    )
    .unwrap();
    fs::write(src.join("orphan_crate_use_importer.rs"), importer_fixture).unwrap();
    fs::write(src.join("orphan_crate_use_target.rs"), target_fixture).unwrap();

    let lib = parse_rust_file(&src.join("lib.rs")).unwrap();
    let importer = parse_rust_file(&src.join("orphan_crate_use_importer.rs")).unwrap();
    let target = parse_rust_file(&src.join("orphan_crate_use_target.rs")).unwrap();

    let parsed: Vec<&ParsedRustFile> = vec![&lib, &importer, &target];
    let g = build_rust_dependency_graph(&parsed);
    let viols = analyze_graph(&g, &Config::rust_defaults());

    assert!(
        !viols.iter().any(|v| v.metric == "orphan_module" && v.unit_name == "orphan_crate_use_target"),
        "Expected orphan_crate_use_target not to be orphan when imported via crate::; got:\n{viols:#?}"
    );
}

#[test]
fn bug_orphan_module_should_not_flag_include_macro_in_rust() {
    // RULE: [Rust] [orphan_module]
    //
    // Hypothesis: `include!("file.rs")` is not recognized as a dependency edge, so the included
    // file (analyzed as its own module) is incorrectly flagged orphan_module.
    //
    // Prediction: orphan_include_target should NOT be orphan when it is included by lib.rs.
    //
    // Test: Copy fixtures into a temp `src/` crate root, build graph, and run orphan analysis.
    use std::fs;
    use tempfile::TempDir;

    let lib_fixture = fs::read_to_string("tests/fake_rust/orphan_include_lib.rs").unwrap();
    let target_fixture = fs::read_to_string("tests/fake_rust/orphan_include_target.rs").unwrap();

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();

    fs::write(src.join("lib.rs"), lib_fixture).unwrap();
    fs::write(src.join("orphan_include_target.rs"), target_fixture).unwrap();

    let lib = parse_rust_file(&src.join("lib.rs")).unwrap();
    let target = parse_rust_file(&src.join("orphan_include_target.rs")).unwrap();

    let parsed: Vec<&ParsedRustFile> = vec![&lib, &target];
    let g = build_rust_dependency_graph(&parsed);
    let viols = analyze_graph(&g, &Config::rust_defaults());

    assert!(
        !viols.iter().any(|v| v.metric == "orphan_module" && v.unit_name == "orphan_include_target"),
        "Expected orphan_include_target not to be orphan when included via include!; got:\n{viols:#?}"
    );
}

