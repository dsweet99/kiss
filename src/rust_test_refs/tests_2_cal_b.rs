use super::*;

#[test]
fn test_apply_import_calibration_skips_binary_crate_src_neighbor() {
    use super::calibration::apply_rust_import_dependency_calibration;
    use crate::graph::DependencyGraph;

    let tmp = tempfile::TempDir::new().unwrap();
    let crates = tmp.path().join("crates/ruff/src");
    std::fs::create_dir_all(&crates).unwrap();
    std::fs::write(crates.join("main.rs"), "fn main() {}\n").unwrap();
    let printer = crates.join("printer.rs");
    std::fs::write(&printer, "pub fn print() {}\n").unwrap();
    let witness = tmp.path().join("witness.rs");
    std::fs::write(&witness, "pub fn seen() {}\n").unwrap();

    let mut graph = DependencyGraph::new();
    let printer_canon = crate::rust_include::canonical_path(&printer);
    let witness_canon = crate::rust_include::canonical_path(&witness);
    graph
        .path_to_module
        .insert(printer_canon.clone(), "printer".into());
    graph.path_to_module.insert(witness_canon, "witness".into());
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
    let mut unreferenced = vec![definitions[1].clone()];
    let witness_refs = HashSet::from(["seen".to_string()]);
    apply_rust_import_dependency_calibration(
        &definitions,
        &mut unreferenced,
        &graph,
        &witness_refs,
    );
    assert_eq!(unreferenced.len(), 1);
    assert_eq!(unreferenced[0].name, "print");
}

#[test]
fn test_apply_import_calibration_skips_single_crate_cli_neighbor() {
    use super::calibration::apply_rust_import_dependency_calibration;
    use crate::graph::DependencyGraph;

    let tmp = tempfile::TempDir::new().unwrap();
    let cli = tmp.path().join("src/cli/exit.rs");
    std::fs::create_dir_all(cli.parent().unwrap()).unwrap();
    std::fs::write(&cli, "pub fn bail() {}\n").unwrap();
    let witness = tmp.path().join("src/lib.rs");
    std::fs::write(&witness, "pub fn seen() {}\n").unwrap();

    let mut graph = DependencyGraph::new();
    let cli_canon = crate::rust_include::canonical_path(&cli);
    let witness_canon = crate::rust_include::canonical_path(&witness);
    graph.path_to_module.insert(cli_canon, "cli".into());
    graph.path_to_module.insert(witness_canon, "witness".into());
    graph.get_or_create_node("cli");
    graph.get_or_create_node("witness");
    graph.add_dependency("witness", "cli");

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
            name: "bail".into(),
            kind: crate::units::CodeUnitKind::Function,
            file: cli,
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
    assert_eq!(unreferenced[0].name, "bail");
}
