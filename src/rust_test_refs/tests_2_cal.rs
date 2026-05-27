use super::*;
use crate::rust_parsing::{parse_rust_file, ParsedRustFile};

#[test]
fn test_path_is_under_tests() {
    assert!(super::path_is_under_tests(Path::new("tests/foo.rs")));
    assert!(!super::path_is_under_tests(Path::new("src/lib.rs")));
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

#[test]
fn test_is_coverage_map_cli_commands_file() {
    use super::calibration;
    assert!(calibration::is_coverage_map_cli_commands_file(Path::new(
        "src/cli/foo.rs"
    )));
    assert!(calibration::is_coverage_map_cli_commands_file(Path::new(
        "src/commands/bar.rs"
    )));
    assert!(!calibration::is_coverage_map_cli_commands_file(Path::new(
        "src/lib.rs"
    )));
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
fn test_apply_import_calibration_credits_neighbor_module() {
    use super::calibration::{
        apply_rust_import_dependency_calibration, binary_entry_paths, module_definition_counts,
        module_has_rust_witness, resolve_mod_child_path,
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
    use super::calibration::collect_fn_names_from_items;
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
    let defs = vec![RustCodeDefinition {
        name: "f".into(),
        kind: crate::units::CodeUnitKind::Function,
        file: PathBuf::from("lib.rs"),
        line: 1,
        end_line: 1,
        impl_for_type: None,
    }];
    assert_eq!(module_definition_counts(&defs, &graph).get("m").copied(), Some(1));
}

#[test]
fn test_resolve_mod_child_path_finds_flat_and_nested() {
    use super::calibration::resolve_mod_child_path;
    let tmp = tempfile::TempDir::new().unwrap();
    let parent = tmp.path().join("parent.rs");
    std::fs::write(&parent, "").unwrap();
    let child_rs = tmp.path().join("child.rs");
    std::fs::write(&child_rs, "").unwrap();
    assert_eq!(
        resolve_mod_child_path(&parent, "child"),
        Some(crate::rust_include::canonical_path(&child_rs))
    );
}

#[test]
fn test_integration_cone_file_paths_and_calibration_map() {
    use super::calibration_map::build_rust_coverage_map_for_calibration;
    use super::calibration::integration_cone_file_paths;

    let tmp = tempfile::TempDir::new().unwrap();
    let main_rs = tmp.path().join("main.rs");
    std::fs::write(&main_rs, "mod child;\nfn main() { child::run(); }\n").unwrap();
    let child_rs = tmp.path().join("child.rs");
    std::fs::write(&child_rs, "pub fn run() {}\n").unwrap();
    let parsed_main = parse_rust_file(&main_rs).unwrap();
    let parsed_child = parse_rust_file(&child_rs).unwrap();
    let files = [&parsed_main, &parsed_child];
    let seeds = vec![crate::rust_include::canonical_path(&main_rs)];
    let cone = integration_cone_file_paths(&files, &seeds, 4);
    assert!(cone.contains(&crate::rust_include::canonical_path(&child_rs)));

    let defs = vec![RustCodeDefinition {
        name: "run".into(),
        kind: crate::units::CodeUnitKind::Function,
        file: child_rs,
        line: 1,
        end_line: 1,
        impl_for_type: None,
    }];
    let usage: PerTestUsage = Vec::new();
    let refs = HashSet::from(["run".to_string()]);
    let map = build_rust_coverage_map_for_calibration(
        &defs,
        &usage,
        &HashMap::new(),
        &HashMap::new(),
        &refs,
    );
    assert!(map.is_empty());
}
