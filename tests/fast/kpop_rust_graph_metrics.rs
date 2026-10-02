use kiss::config::Config;
use kiss::graph::analyze_graph;
use kiss::rust_graph::build_rust_dependency_graph;
use kiss::rust_parsing::{ParsedRustFile, parse_rust_file};
use std::path::Path;

fn parse_rs(path: &Path) -> ParsedRustFile {
    parse_rust_file(path).expect("parse rust fixture")
}

#[test]
fn bug_rust_indirect_dependencies_should_not_count_external_imports() {
    let a = parse_rs(Path::new("tests/fake_rust/rust_graph_ext_a.rs"));
    let b = parse_rs(Path::new("tests/fake_rust/rust_graph_ext_b.rs"));
    let parsed: Vec<&ParsedRustFile> = vec![&a, &b];
    let g = build_rust_dependency_graph(&parsed);

    let m = g.module_metrics("fake_rust.rust_graph_ext_a");
    assert_eq!(m.fan_out, 1);
    assert_eq!(m.indirect_dependencies, 0);
}

#[test]
fn bug_orphan_module_should_not_flag_crate_use_imports_in_rust() {
    use std::fs;
    use tempfile::TempDir;

    let importer_fixture =
        fs::read_to_string("tests/fake_rust/orphan_crate_use_importer.rs").unwrap();
    let target_fixture = fs::read_to_string("tests/fake_rust/orphan_crate_use_target.rs").unwrap();

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();

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
        !viols
            .iter()
            .any(|v| v.metric == "orphan_module" && v.unit_name == "orphan_crate_use_target"),
        "Expected orphan_crate_use_target not to be orphan when imported via crate::; got:\n{viols:#?}"
    );
}

#[test]
fn bug_orphan_module_should_not_flag_include_macro_in_rust() {
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
        !viols
            .iter()
            .any(|v| v.metric == "orphan_module" && v.unit_name == "orphan_include_target"),
        "Expected orphan_include_target not to be orphan when included via include!; got:\n{viols:#?}"
    );
}

#[test]
fn include_inc_fragment_not_orphan_when_included() {
    use std::fs;
    use tempfile::TempDir;

    let lib_fixture = fs::read_to_string("tests/fake_rust/include_inc_lib.rs").unwrap();
    let inc_fixture = fs::read_to_string("tests/fake_rust/include_inc_fragment.inc").unwrap();

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("lib.rs"), lib_fixture).unwrap();
    fs::write(src.join("include_inc_fragment.inc"), inc_fixture).unwrap();

    let lib = parse_rust_file(&src.join("lib.rs")).unwrap();
    let frag = parse_rust_file(&src.join("include_inc_fragment.inc")).unwrap();
    let parsed: Vec<&ParsedRustFile> = vec![&lib, &frag];
    let g = build_rust_dependency_graph(&parsed);
    let viols = analyze_graph(&g, &Config::rust_defaults());

    assert!(
        !viols.iter().any(|v| {
            v.metric == "orphan_module"
                && v.file
                    .file_name()
                    .is_some_and(|n| n == "include_inc_fragment.inc")
        }),
        "included .inc fragment should not be orphan; got:\n{viols:#?}"
    );
}

#[test]
fn include_rollup_counts_fragment_statements_on_includer() {
    use kiss::rust_counts::analyze_rust_file_include_rollup;
    use std::fs;
    use tempfile::TempDir;

    let lib_fixture = fs::read_to_string("tests/fake_rust/include_inc_lib.rs").unwrap();
    let inc_fixture = fs::read_to_string("tests/fake_rust/include_inc_fragment.inc").unwrap();

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("lib.rs"), lib_fixture).unwrap();
    fs::write(src.join("include_inc_fragment.inc"), inc_fixture).unwrap();

    let lib = parse_rust_file(&src.join("lib.rs")).unwrap();
    let frag = parse_rust_file(&src.join("include_inc_fragment.inc")).unwrap();

    let mut cfg = Config::rust_defaults();
    cfg.statements_per_file = 2;

    let viols = analyze_rust_file_include_rollup(&lib, &[&frag], &cfg);
    assert!(
        viols.iter().any(|v| {
            v.metric == "statements_per_file"
                && v.file.file_name().is_some_and(|n| n == "lib.rs")
                && v.message.contains("include_inc_fragment.inc")
        }),
        "rollup violation should cite fragment path; got:\n{viols:#?}"
    );
}
