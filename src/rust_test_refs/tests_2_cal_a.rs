use super::*;
use crate::graph::DependencyGraph;
use crate::rust_parsing::parse_rust_file;

#[test]
fn test_path_is_under_tests() {
    assert!(super::path_is_under_tests(Path::new("tests/foo.rs")));
    assert!(!super::path_is_under_tests(Path::new("src/lib.rs")));
}

#[test]
fn test_is_subprocess_integration_test_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("tests")).unwrap();
    let integration = tmp.path().join("tests/spawn.rs");
    std::fs::write(
        &integration,
        "fn t() { let _ = std::process::Command::new(\"app\"); }\n",
    )
    .unwrap();
    let parsed = parse_rust_file(&integration).unwrap();
    assert!(super::calibration::is_subprocess_integration_test_file(&parsed));

    let unit = tmp.path().join("tests/unit.rs");
    std::fs::write(&unit, "#[test]\nfn ok() { assert!(true); }\n").unwrap();
    let parsed_unit = parse_rust_file(&unit).unwrap();
    assert!(!super::calibration::is_subprocess_integration_test_file(&parsed_unit));
}

#[test]
fn test_has_non_subprocess_integration_tests() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("tests")).unwrap();
    std::fs::write(
        tmp.path().join("tests/spawn.rs"),
        "fn t() { let _ = std::process::Command::new(\"app\"); }\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("tests/unit.rs"), "#[test]\nfn ok() {}\n").unwrap();
    let spawn = parse_rust_file(&tmp.path().join("tests/spawn.rs")).unwrap();
    let unit = parse_rust_file(&tmp.path().join("tests/unit.rs")).unwrap();
    let refs = vec![&spawn, &unit];
    assert!(super::calibration::has_non_subprocess_integration_tests(&refs));

    let spawn_only = vec![&spawn];
    assert!(!super::calibration::has_non_subprocess_integration_tests(&spawn_only));
}

#[test]
fn test_has_colocated_src_integration_tests() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    let tests_dir = tmp.path().join("tests");
    std::fs::create_dir_all(&tests_dir).unwrap();
    std::fs::write(
        tests_dir.join("integration.rs"),
        "fn t() { std::process::Command::new(\"app\"); }\n",
    )
    .unwrap();
    for i in 0..8 {
        std::fs::write(
            src.join(format!("feature_{i}_tests.rs")),
            "#[test]\nfn ok() { assert!(true); }\n",
        )
        .unwrap();
    }
    let parsed: Vec<_> = std::fs::read_dir(&src)
        .unwrap()
        .chain(std::fs::read_dir(&tests_dir).unwrap())
        .filter_map(Result::ok)
        .filter_map(|e| parse_rust_file(&e.path()).ok())
        .collect();
    let refs: Vec<_> = parsed.iter().collect();
    assert!(super::calibration::has_colocated_src_integration_tests(&refs));
}

#[test]
fn test_seed_binary_entry_roots_finds_bin_main() {
    let tmp = tempfile::TempDir::new().unwrap();
    let bin_dir = tmp.path().join("src/bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let bin_rs = bin_dir.join("app.rs");
    std::fs::write(&bin_rs, "fn run() {}\n").unwrap();
    let parsed_bin = parse_rust_file(&bin_rs).unwrap();
    let mut refs = HashSet::new();
    super::seed_binary_entry_roots(&[&parsed_bin], &mut refs);
    assert!(refs.contains("run"));
}

#[test]
fn test_seed_binary_entry_roots_from_item() {
    let ast: syn::File = syn::parse_str("fn main() {}\nfn run() {}\n").unwrap();
    let mut refs = HashSet::new();
    for item in &ast.items {
        super::seed_binary_entry_roots_from_item(item, &mut refs);
    }
    assert!(refs.contains("main"));
    assert!(refs.contains("run"));
}

#[test]
fn test_expand_coverage_references_to_fixpoint_direct() {
    let tmp = tempfile::TempDir::new().unwrap();
    let prod = tmp.path().join("lib.rs");
    std::fs::write(
        &prod,
        "pub fn a() { b(); }\npub fn b() { c(); }\npub fn c() {}\n",
    )
    .unwrap();
    let parsed = parse_rust_file(&prod).unwrap();
    let mut refs = HashSet::from(["a".to_string()]);
    super::expand_coverage_references_to_fixpoint(&[&parsed], &mut refs);
    assert!(refs.contains("c"));
}

#[test]
fn test_expand_coverage_references_one_hop_direct() {
    let tmp = tempfile::TempDir::new().unwrap();
    let prod = tmp.path().join("lib.rs");
    std::fs::write(&prod, "pub fn entry() { helper(); }\npub fn helper() {}\n").unwrap();
    let parsed = parse_rust_file(&prod).unwrap();
    let mut refs = HashSet::from(["entry".to_string()]);
    super::expand_coverage_references_one_hop(&[&parsed], &mut refs);
    assert!(refs.contains("helper"));
}

#[test]
fn test_expand_one_hop_from_item_direct() {
    let ast: syn::File = syn::parse_str("fn entry() { leaf(); }\nfn leaf() {}\n").unwrap();
    let refs = HashSet::from(["entry".to_string()]);
    let mut added = HashSet::new();
    for item in &ast.items {
        super::expand_one_hop_from_item(item, &refs, &mut added);
    }
    assert!(added.contains("leaf"));
}

#[test]
fn test_merge_one_hop_refs_direct() {
    let refs = HashSet::from(["seen".to_string()]);
    let mut added = HashSet::new();
    let body = HashSet::from(["seen".to_string(), "new".to_string()]);
    super::merge_one_hop_refs(body, &refs, &mut added);
    assert!(!added.contains("seen"));
    assert!(added.contains("new"));
}

#[test]
fn test_expand_coverage_one_hop_through_impl_method() {
    let tmp = tempfile::TempDir::new().unwrap();
    let prod = tmp.path().join("lib.rs");
    std::fs::write(
        &prod,
        "struct S;\nimpl S {\n    pub fn caller() { helper(); }\n    fn helper() {}\n}\n",
    )
    .unwrap();
    let test = tmp.path().join("s_test.rs");
    std::fs::write(
        &test,
        "#[test]\nfn t() { S::caller(); }\n",
    )
    .unwrap();
    let parsed_prod = parse_rust_file(&prod).unwrap();
    let parsed_test = parse_rust_file(&test).unwrap();
    let cal = analyze_rust_test_refs_for_coverage_map(&[&parsed_prod, &parsed_test], None);
    assert!(
        !cal
            .unreferenced
            .iter()
            .any(|d| d.name == "helper"),
        "one-hop through impl method body should cover helper"
    );
}

fn printer_import_fixture() -> (
    tempfile::TempDir,
    DependencyGraph,
    Vec<RustCodeDefinition>,
) {
    let tmp = tempfile::TempDir::new().unwrap();
    let crates = tmp.path().join("crates/ruff/src");
    std::fs::create_dir_all(&crates).unwrap();
    std::fs::write(crates.join("main.rs"), "fn main() {}\n").unwrap();
    let printer = crates.join("printer.rs");
    std::fs::write(&printer, "pub fn print() {}\n").unwrap();
    let witness = tmp.path().join("witness.rs");
    std::fs::write(&witness, "pub fn seen() {}\n").unwrap();
    let mut graph = DependencyGraph::new();
    graph
        .path_to_module
        .insert(crate::rust_include::canonical_path(&printer), "printer".into());
    graph
        .path_to_module
        .insert(crate::rust_include::canonical_path(&witness), "witness".into());
    graph.get_or_create_node("printer");
    graph.get_or_create_node("witness");
    graph.add_dependency("witness", "printer");
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
            name: "print".into(),
            kind: crate::units::CodeUnitKind::Function,
            file: printer,
            line: 1,
            end_line: 1,
            impl_for_type: None,
        },
    ];
    (tmp, graph, definitions)
}

#[test]
fn test_import_cal_does_not_credit_binary_printer_neighbor() {
    use super::calibration::apply_rust_import_dependency_calibration;
    let (_tmp, graph, definitions) = printer_import_fixture();
    let parsed: Vec<_> = definitions
        .iter()
        .map(|d| parse_rust_file(&d.file).unwrap())
        .collect();
    let refs: Vec<_> = parsed.iter().collect();
    let analysis = analyze_rust_test_refs_for_coverage_map(&refs, Some(&graph));
    let printer_covered = analysis
        .definitions
        .iter()
        .filter(|d| d.name == "print")
        .filter(|d| {
            !analysis
                .unreferenced
                .iter()
                .any(|u| u.file == d.file && u.name == d.name && u.line == d.line)
        })
        .count();
    assert_eq!(printer_covered, 0);
    let mut unreferenced = vec![definitions[1].clone()];
    apply_rust_import_dependency_calibration(
        &definitions,
        &mut unreferenced,
        &graph,
        &HashSet::from(["seen".to_string()]),
    );
    assert_eq!(unreferenced.len(), 1);
    assert_eq!(unreferenced[0].name, "print");
}
