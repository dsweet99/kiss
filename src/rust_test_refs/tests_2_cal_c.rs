use super::*;
use crate::rust_parsing::parse_rust_file;

#[test]
fn test_module_is_binary_crate_src_only_paths() {
    use super::calibration::module_is_binary_crate_src_only;
    use crate::graph::DependencyGraph;
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("crates/app/src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("main.rs"), "fn main() {}\n").unwrap();
    let printer = src.join("printer.rs");
    std::fs::write(&printer, "pub fn print() {}\n").unwrap();
    let lib = src.join("lib.rs");
    std::fs::write(&lib, "pub fn core() {}\n").unwrap();
    let mut graph = DependencyGraph::new();
    graph.path_to_module.insert(crate::rust_include::canonical_path(&printer), "printer".into());
    graph.path_to_module.insert(crate::rust_include::canonical_path(&lib), "mixed".into());
    let defs = vec![
        RustCodeDefinition {
            name: "print".into(),
            kind: crate::units::CodeUnitKind::Function,
            file: printer,
            line: 1,
            end_line: 1,
            impl_for_type: None,
        },
        RustCodeDefinition {
            name: "core".into(),
            kind: crate::units::CodeUnitKind::Function,
            file: lib,
            line: 1,
            end_line: 1,
            impl_for_type: None,
        },
    ];
    assert!(module_is_binary_crate_src_only("printer", &defs, &graph));
    assert!(!module_is_binary_crate_src_only("mixed", &defs, &graph));
}

#[test]
fn test_module_is_cli_and_settings_paths() {
    use super::calibration::{module_is_rule_settings_only, module_is_single_crate_cli_only};
    use crate::graph::DependencyGraph;
    let tmp = tempfile::TempDir::new().unwrap();
    let cli = tmp.path().join("src/cli/exit.rs");
    std::fs::create_dir_all(cli.parent().unwrap()).unwrap();
    std::fs::write(&cli, "pub fn bail() {}\n").unwrap();
    let settings = tmp.path().join("crates/ruff_linter/src/rules/flake8_x/settings.rs");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(&settings, "pub struct S;\n").unwrap();
    let mut graph = DependencyGraph::new();
    graph.path_to_module.insert(crate::rust_include::canonical_path(&cli), "cli".into());
    graph
        .path_to_module
        .insert(crate::rust_include::canonical_path(&settings), "settings".into());
    let defs = vec![
        RustCodeDefinition {
            name: "bail".into(),
            kind: crate::units::CodeUnitKind::Function,
            file: cli,
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
    assert!(module_is_single_crate_cli_only("cli", &defs, &graph));
    assert!(module_is_rule_settings_only("settings", &defs, &graph));
}

#[test]
fn test_module_is_helpers_empty_module() {
    use super::calibration::{apply_rust_import_dependency_calibration, module_definition_counts};
    use crate::graph::DependencyGraph;

    let mut graph = DependencyGraph::new();
    graph.get_or_create_node("empty");
    graph.get_or_create_node("witness");
    graph.add_dependency("witness", "empty");
    let definitions = vec![RustCodeDefinition {
        name: "seen".into(),
        kind: crate::units::CodeUnitKind::Function,
        file: PathBuf::from("witness.rs"),
        line: 1,
        end_line: 1,
        impl_for_type: None,
    }];
    assert!(module_definition_counts(&definitions, &graph).is_empty());
    let mut unreferenced = Vec::new();
    apply_rust_import_dependency_calibration(
        &definitions,
        &mut unreferenced,
        &graph,
        &HashSet::from(["seen".to_string()]),
    );
    assert!(unreferenced.is_empty());
}

#[test]
fn test_apply_import_calibration_mixed_binary_module_not_all_src_root() {
    use super::calibration::apply_rust_import_dependency_calibration;
    use crate::graph::DependencyGraph;

    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("crates/ruff/src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(src.join("lib.rs"), "pub fn core() {}\n").unwrap();
    std::fs::write(src.join("printer.rs"), "pub fn print() {}\n").unwrap();
    let witness = tmp.path().join("witness.rs");
    std::fs::write(&witness, "pub fn seen() {}\n").unwrap();

    let mut graph = DependencyGraph::new();
    let lib_canon = crate::rust_include::canonical_path(&src.join("lib.rs"));
    let witness_canon = crate::rust_include::canonical_path(&witness);
    graph.path_to_module.insert(lib_canon.clone(), "mixed".into());
    graph
        .path_to_module
        .insert(crate::rust_include::canonical_path(&src.join("printer.rs")), "mixed".into());
    graph.path_to_module.insert(witness_canon, "witness".into());
    graph.get_or_create_node("mixed");
    graph.get_or_create_node("witness");
    graph.add_dependency("witness", "mixed");

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
            name: "core".into(),
            kind: crate::units::CodeUnitKind::Function,
            file: src.join("lib.rs"),
            line: 1,
            end_line: 1,
            impl_for_type: None,
        },
        RustCodeDefinition {
            name: "print".into(),
            kind: crate::units::CodeUnitKind::Function,
            file: src.join("printer.rs"),
            line: 1,
            end_line: 1,
            impl_for_type: None,
        },
    ];
    let mut unreferenced = vec![definitions[1].clone(), definitions[2].clone()];
    apply_rust_import_dependency_calibration(
        &definitions,
        &mut unreferenced,
        &graph,
        &HashSet::from(["seen".to_string()]),
    );
    assert!(
        unreferenced.iter().any(|d| d.name == "print"),
        "binary src root still unreferenced"
    );
    assert!(
        !unreferenced.iter().any(|d| d.name == "core"),
        "non-binary file in mixed module may be import-cal credited"
    );
}

#[test]
fn test_apply_import_calibration_credits_neighbor_module() {
    use super::calibration::{
        apply_rust_import_dependency_calibration, module_definition_counts,
        module_has_rust_witness,
    };
    use crate::graph::DependencyGraph;

    let tmp = tempfile::TempDir::new().unwrap();
    let witness_path = tmp.path().join("witness.rs");
    let neighbor_path = tmp.path().join("neighbor.rs");
    std::fs::write(&witness_path, "pub fn seen() {}\n").unwrap();
    std::fs::write(&neighbor_path, "pub fn helper() {}\npub fn hidden() {}\n").unwrap();

    let mut graph = DependencyGraph::new();
    let witness_canon = crate::rust_include::canonical_path(&witness_path);
    let neighbor_canon = crate::rust_include::canonical_path(&neighbor_path);
    graph.path_to_module.insert(witness_canon, "witness".into());
    graph.path_to_module.insert(neighbor_canon, "neighbor".into());
    graph.get_or_create_node("witness");
    graph.get_or_create_node("neighbor");
    graph.add_dependency("witness", "neighbor");

    let definitions = vec![
        RustCodeDefinition {
            name: "seen".into(),
            kind: crate::units::CodeUnitKind::Function,
            file: witness_path,
            line: 1,
            end_line: 1,
            impl_for_type: None,
        },
        RustCodeDefinition {
            name: "helper".into(),
            kind: crate::units::CodeUnitKind::Function,
            file: neighbor_path.clone(),
            line: 1,
            end_line: 1,
            impl_for_type: None,
        },
        RustCodeDefinition {
            name: "hidden".into(),
            kind: crate::units::CodeUnitKind::Function,
            file: neighbor_path,
            line: 2,
            end_line: 2,
            impl_for_type: None,
        },
    ];
    let counts = module_definition_counts(&definitions, &graph);
    assert_eq!(counts.get("witness").copied(), Some(1));
    let witness = HashSet::from(["seen".to_string(), "helper".to_string()]);
    assert!(module_has_rust_witness("witness", &definitions, &graph, &witness));

    let mut unreferenced = vec![definitions[2].clone()];
    apply_rust_import_dependency_calibration(
        &definitions,
        &mut unreferenced,
        &graph,
        &witness,
    );
    assert!(unreferenced.is_empty());
}

#[test]
fn test_resolve_mod_child_and_binary_entry_paths() {
    use super::calibration::{binary_entry_paths, resolve_mod_child_path};
    let tmp = tempfile::TempDir::new().unwrap();
    let parent = tmp.path().join("src");
    std::fs::create_dir_all(&parent).unwrap();
    let flat = parent.join("child.rs");
    std::fs::write(&flat, "pub fn run() {}\n").unwrap();
    assert_eq!(
        resolve_mod_child_path(&parent.join("lib.rs"), "child"),
        Some(crate::rust_include::canonical_path(&flat))
    );
    let bin_dir = tmp.path().join("src/bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let main_rs = bin_dir.join("app.rs");
    std::fs::write(&main_rs, "fn main() {}\n").unwrap();
    let parsed = parse_rust_file(&main_rs).unwrap();
    assert_eq!(binary_entry_paths(&[&parsed]).len(), 1);
}

#[test]
fn test_small_module_stem_expands_public_fns() {
    use super::calibration::expand_small_module_defs_from_stem_refs;
    use std::io::Write as _;

    let mut msg = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    write!(msg, "pub fn a() {{}}\npub fn b() {{}}\n").unwrap();
    let parsed_msg = parse_rust_file(msg.path()).unwrap();
    let stem = parsed_msg.path.file_stem().unwrap().to_str().unwrap();
    let mut refs = HashSet::from([stem.to_string()]);
    expand_small_module_defs_from_stem_refs(&[&parsed_msg], &mut refs);
    assert!(refs.contains("a"));
    assert!(refs.contains("b"));
}

#[test]
fn test_collect_fn_names_skips_private_and_test_fns() {
    use super::collect_fn_names_from_items;
    let ast: syn::File =
        syn::parse_str("fn public() {}\nfn _private() {}\n#[test]\nfn t() {}\n").unwrap();
    let mut names = Vec::new();
    collect_fn_names_from_items(&ast.items, &mut names);
    assert!(names.contains(&"public".to_string()));
    assert!(!names.contains(&"_private".to_string()));
    assert!(!names.contains(&"t".to_string()));
}

#[test]
fn test_impl_self_type_name_and_module_counts() {
    use super::calibration::{impl_self_type_name, module_definition_counts};
    use crate::graph::DependencyGraph;

    let ty: syn::Type = syn::parse_str("crate::Foo").unwrap();
    assert_eq!(impl_self_type_name(&ty).as_deref(), Some("Foo"));

    let mut graph = DependencyGraph::new();
    graph.path_to_module.insert(PathBuf::from("lib.rs"), "m".into());
    assert_eq!(
        module_definition_counts(
            &[RustCodeDefinition {
                name: "f".into(),
                kind: crate::units::CodeUnitKind::Function,
                file: PathBuf::from("lib.rs"),
                line: 1,
                end_line: 1,
                impl_for_type: None,
            }],
            &graph
        )
        .get("m")
        .copied(),
        Some(1)
    );
    let tmp = tempfile::TempDir::new().unwrap();
    let lib = tmp.path().join("lib.rs");
    std::fs::write(&lib, "fn a() {}\nfn b() {}\n").unwrap();
    let file = crate::rust_include::canonical_path(&lib);
    graph.path_to_module.insert(file.clone(), "m2".into());
    let two = vec![
        RustCodeDefinition {
            name: "a".into(),
            kind: crate::units::CodeUnitKind::Function,
            file: file.clone(),
            line: 1,
            end_line: 1,
            impl_for_type: None,
        },
        RustCodeDefinition {
            name: "b".into(),
            kind: crate::units::CodeUnitKind::Function,
            file,
            line: 2,
            end_line: 2,
            impl_for_type: None,
        },
    ];
    assert_eq!(module_definition_counts(&two, &graph).get("m2").copied(), Some(2));
    assert!(module_definition_counts(&[], &graph).is_empty());
}
