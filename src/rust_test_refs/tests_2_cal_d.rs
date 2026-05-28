use super::*;
use crate::graph::DependencyGraph;
use crate::rust_parsing::{parse_rust_file, ParsedRustFile};

#[test]
fn test_ruff_printer_path_is_binary_crate_src_and_stays_unref() {
    use super::calibration_map::is_coverage_map_binary_crate_src_root;
    let printer = PathBuf::from("/home/dsweet/Projects/repos/ruff/crates/ruff/src/printer.rs");
    if printer.is_file() {
        assert!(is_coverage_map_binary_crate_src_root(&printer));
        let parsed = parse_rust_file(&printer).unwrap();
        let lib = printer.parent().unwrap().join("lib.rs");
        let main = printer.parent().unwrap().join("main.rs");
        let parsed_lib = parse_rust_file(&lib).unwrap();
        let parsed_main = parse_rust_file(&main).unwrap();
        let files: Vec<&ParsedRustFile> = vec![&parsed_lib, &parsed_main, &parsed];
        let graph = crate::rust_graph::build_rust_dependency_graph(&files);
        let cal = analyze_rust_test_refs_for_coverage_map(&files, Some(&graph));
        let printer_unref: Vec<_> = cal
            .unreferenced
            .iter()
            .filter(|d| d.file == printer)
            .collect();
        assert!(
            !printer_unref.is_empty(),
            "binary src printer.rs defs should be unreferenced for coverage map"
        );
    }
}

#[test]
fn test_is_coverage_map_cli_surface_paths() {
    use super::calibration;
    use super::calibration_map::is_coverage_map_binary_crate_src_root;
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("crates").join("app").join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("main.rs"), "fn main() {}\n").unwrap();
    assert!(is_coverage_map_binary_crate_src_root(&src.join("args.rs")));
    assert!(!is_coverage_map_binary_crate_src_root(&src.join("lib.rs")));
    let flat = tmp.path().join("src");
    std::fs::create_dir_all(&flat).unwrap();
    std::fs::write(flat.join("main.rs"), "fn main() {}\n").unwrap();
    assert!(!is_coverage_map_binary_crate_src_root(&flat.join("args.rs")));
    assert!(calibration::is_coverage_map_cli_commands_file(Path::new(
        "src/cli/foo.rs"
    )));
    assert!(calibration::is_coverage_map_cli_commands_file(Path::new(
        "src/commands/bar.rs"
    )));
    assert!(!calibration::is_coverage_map_cli_commands_file(Path::new(
        "src/lib.rs"
    )));
    assert!(calibration::is_calibration_excluded_file(Path::new(
        "crates/ty_server/src/lib.rs"
    )));
}

#[test]
fn test_integration_cone_witness_refs_expands_from_main() {
    use super::calibration::integration_cone_witness_refs;
    let tmp = tempfile::TempDir::new().unwrap();
    let tests_dir = tmp.path().join("tests");
    std::fs::create_dir_all(&tests_dir).unwrap();
    std::fs::write(
        tests_dir.join("runner.rs"),
        "fn t() { let _ = std::process::Command::new(\"kiss\"); }\n",
    )
    .unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("main.rs"),
        "mod child;\nfn main() { child::run(); }\n",
    )
    .unwrap();
    std::fs::write(src.join("child.rs"), "pub fn run() { leaf(); }\npub fn leaf() {}\n").unwrap();
    let parsed: Vec<_> = [tests_dir.join("runner.rs"), src.join("main.rs"), src.join("child.rs")]
        .iter()
        .map(|p| parse_rust_file(p).unwrap())
        .collect();
    let refs: Vec<_> = parsed.iter().collect();
    let cone_refs = integration_cone_witness_refs(&refs);
    assert!(cone_refs.contains("main"));
    assert!(cone_refs.contains("run"));
    assert!(cone_refs.contains("leaf"));
    let lib = src.join("lib.rs");
    std::fs::write(&lib, "pub fn only() {}\n").unwrap();
    let parsed_lib = parse_rust_file(&lib).unwrap();
    assert!(integration_cone_witness_refs(&[&parsed_lib]).is_empty());
}

#[test]
fn test_integration_cone_with_runner_and_binary_entry() {
    use super::calibration;
    let tmp = tempfile::TempDir::new().unwrap();
    let tests_dir = tmp.path().join("tests");
    std::fs::create_dir_all(&tests_dir).unwrap();
    let integration = tests_dir.join("runner.rs");
    std::fs::write(
        &integration,
        "fn t() { let _ = std::process::Command::new(\"kiss\"); }\n",
    )
    .unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let main_rs = src.join("main.rs");
    std::fs::write(&main_rs, "mod child;\nfn main() { child::run(); }\n").unwrap();
    let child_rs = src.join("child.rs");
    std::fs::write(&child_rs, "pub fn run() {}\n").unwrap();
    let parsed_runner = parse_rust_file(&integration).unwrap();
    let parsed_main = parse_rust_file(&main_rs).unwrap();
    let parsed_child = parse_rust_file(&child_rs).unwrap();
    let files: Vec<&ParsedRustFile> = vec![&parsed_runner, &parsed_main, &parsed_child];
    let seeds = calibration::integration_cone_files_for(&files);
    assert!(seeds.contains(&crate::rust_include::canonical_path(&main_rs)));
    let mut refs = HashSet::from(["main".to_string()]);
    calibration::expand_integration_cone_witnesses(&files, &mut refs);
    assert!(refs.contains("run"));
    calibration::expand_coverage_map_witnesses(&files, &mut refs);
    assert!(refs.contains("main"));
}

#[test]
fn test_expand_coverage_one_hop_from_test_call() {
    let tmp = tempfile::TempDir::new().unwrap();
    let prod = tmp.path().join("lib.rs");
    std::fs::write(
        &prod,
        "pub fn entry() { helper_a(); helper_b(); }\npub fn helper_a() {}\npub fn helper_b() {}\n",
    )
    .unwrap();
    let test = tmp.path().join("entry_test.rs");
    std::fs::write(&test, "#[test]\nfn t() { entry(); helper_a(); }\n").unwrap();
    let parsed_prod = parse_rust_file(&prod).unwrap();
    let parsed_test = parse_rust_file(&test).unwrap();
    let cal = analyze_rust_test_refs_for_coverage_map(&[&parsed_prod, &parsed_test], None);
    assert!(
        !cal
            .unreferenced
            .iter()
            .any(|d| d.name == "helper_b"),
        "one-hop should cover helper_b when entry is called from a test"
    );
}

#[test]
fn test_apply_import_calibration_retains_rule_settings() {
    use super::calibration::apply_rust_import_dependency_calibration;

    let tmp = tempfile::TempDir::new().unwrap();
    let settings = tmp.path().join("crates/ruff_linter/src/rules/flake8_x/settings.rs");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(&settings, "pub struct S;\n").unwrap();
    let witness = tmp.path().join("witness.rs");
    std::fs::write(&witness, "pub fn seen() {}\n").unwrap();

    let mut graph = DependencyGraph::new();
    let settings_canon = crate::rust_include::canonical_path(&settings);
    let witness_canon = crate::rust_include::canonical_path(&witness);
    graph
        .path_to_module
        .insert(settings_canon, "settings".into());
    graph.path_to_module.insert(witness_canon, "witness".into());
    graph.get_or_create_node("settings");
    graph.get_or_create_node("witness");
    graph.add_dependency("witness", "settings");

    let definitions = vec![
        RustCodeDefinition {
            name: "seen".into(),
            kind: crate::units::CodeUnitKind::Function,
            file: witness,
            line: 1,
            end_line: 1,
            impl_for_type: None,
        },
        RustCodeDefinition {
            name: "S".into(),
            kind: crate::units::CodeUnitKind::Struct,
            file: settings,
            line: 1,
            end_line: 1,
            impl_for_type: None,
        },
    ];
    let mut unreferenced = vec![definitions[1].clone()];
    apply_rust_import_dependency_calibration(
        &definitions,
        &mut unreferenced,
        &graph,
        &HashSet::from(["seen".to_string()]),
    );
    assert_eq!(unreferenced.len(), 1);
}
