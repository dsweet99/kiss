use super::*;
use crate::rust_parsing::parse_rust_file;
use crate::rust_test_refs::analyze_rust_test_refs;

#[test]
fn direct_weighted_paths_exercised() {
    let tmp = tempfile::TempDir::new().unwrap();
    let handlers = tmp.path().join("crate/handlers");
    std::fs::create_dir_all(&handlers).unwrap();
    std::fs::write(
        handlers.join("mod.rs"),
        "pub fn run(seed: u64) -> u64 { seed + 1 }\n",
    )
    .unwrap();
    let lib = tmp.path().join("lib.rs");
    std::fs::write(
        &lib,
        "pub mod handlers;\npub use handlers::run;\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn dispatch() { let _ = crate::handlers::run(0); }\n}\n",
    )
    .unwrap();
    let parsed_lib = parse_rust_file(&lib).unwrap();
    let parsed_handler = parse_rust_file(&handlers.join("mod.rs")).unwrap();
    let refs: Vec<_> = [&parsed_lib, &parsed_handler].into_iter().collect();
    let analysis = analyze_rust_test_refs(&refs, None);
    let weighted = compute_rs_weighted_file_pcts(&analysis, &refs);
    assert!(weighted.get(&lib).copied().unwrap_or(0) > 0);
    assert!(weighted.get(&parsed_handler.path).copied().unwrap_or(0) > 0);
}

#[test]
fn locate_fn_finds_impl_method() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("worker.rs");
    std::fs::write(
        &src,
        "pub struct Worker;\nimpl Worker {\n    pub fn run(&self) -> u32 { 1 }\n}\n",
    )
    .unwrap();
    let parsed = parse_rust_file(&src).unwrap();
    let def = analyze_rust_test_refs(&[&parsed], None)
        .definitions
        .into_iter()
        .find(|d| d.name == "run")
        .expect("run def");
    assert!(locate_fn(&parsed, &def).is_some());
}

#[test]
fn import_surface_helpers_accept_synthetic_inputs() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("types.rs");
    std::fs::write(
        &src,
        "pub fn free_fn() -> u32 { 1 }\npub struct Type;\nimpl Type {\n    pub fn new() -> Self { Type }\n    pub fn run(&self) -> u32 { 1 }\n}\n",
    )
    .unwrap();
    let parsed = parse_rust_file(&src).unwrap();
    let refs: Vec<_> = [&parsed].into_iter().collect();
    let analysis = analyze_rust_test_refs(&refs, None);
    let unref_set: std::collections::HashSet<_> = analysis
        .unreferenced
        .iter()
        .map(|d| (&d.file, d.name.as_str()))
        .collect();
    let free_def = analysis
        .definitions
        .iter()
        .find(|d| d.name == "free_fn")
        .expect("free_fn");
    let impl_def = analysis
        .definitions
        .iter()
        .find(|d| d.name == "run")
        .expect("run");
    let metrics = crate::rust_fn_metrics::RustFunctionMetrics {
        statements: 4,
        arguments: 1,
        max_indentation: 1,
        nested_function_depth: 0,
        returns: 1,
        branches: 1,
        local_variables: 1,
        bool_parameters: 0,
        attributes: 0,
        calls: 0,
    };
    let parsed_by_path = std::collections::HashMap::from([(parsed.path.clone(), &parsed)]);
    assert_eq!(
        rs_module_import_surface_credit(&analysis, free_def, &metrics, &[], &parsed_by_path),
        Some(0.0)
    );
    assert_eq!(
        rs_module_import_surface_credit(&analysis, impl_def, &metrics, &[], &parsed_by_path),
        None
    );
    assert_eq!(
        rs_import_surface_credit(&analysis, impl_def, &metrics, &[], &parsed_by_path),
        0.0
    );
    assert!(impl_type_covering_tests(&analysis, &unref_set, impl_def).is_none());
}

#[test]
fn locate_fn_finds_nested_mod_function() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("nested.rs");
    std::fs::write(
        &src,
        "mod inner { pub fn compute(n: i32) -> i32 { n + 1 } }\n",
    )
    .unwrap();
    let parsed = parse_rust_file(&src).unwrap();
    let def = analyze_rust_test_refs(&[&parsed], None)
        .definitions
        .into_iter()
        .find(|d| d.name == "compute")
        .expect("compute");
    let located = locate_fn(&parsed, &def).expect("located");
    assert_eq!(located.block.stmts.len(), 1);
}

#[test]
fn direct_accumulate_and_locate_helpers() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("lib.rs");
    std::fs::write(
        &src,
        "pub fn used() -> i32 { 1 }\npub fn unused() -> i32 { 2 }\npub use used;\npub use unused;\n",
    )
    .unwrap();
    let parsed = parse_rust_file(&src).unwrap();
    let refs: Vec<_> = [&parsed].into_iter().collect();
    let analysis = analyze_rust_test_refs(&refs, None);
    let mut by_file = std::collections::HashMap::new();
    let unref_set: std::collections::HashSet<_> = analysis
        .unreferenced
        .iter()
        .map(|d| (&d.file, d.name.as_str()))
        .collect();
    accumulate_pub_use_export_mass(&parsed, &analysis, &unref_set, &mut by_file);
    assert!(by_file.contains_key(&src));
    let file: syn::File = syn::parse_str("use a::{b, c};").unwrap();
    if let syn::Item::Use(u) = &file.items[0] {
        let names = flatten_use_tree_names(&u.tree);
        assert_eq!(names, vec!["b", "c"]);
    }
}

#[test]
fn locate_in_item_finds_mod_inline_function() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("mix.rs");
    std::fs::write(
        &src,
        "mod inner { pub fn nested(n: i32) -> i32 { n + 1 } }\n",
    )
    .unwrap();
    let parsed = parse_rust_file(&src).unwrap();
    let def = analyze_rust_test_refs(&[&parsed], None)
        .definitions
        .into_iter()
        .find(|d| d.name == "nested")
        .expect("nested");
    assert!(locate_in_item(&parsed.ast.items[0], &def).is_some());
}

#[test]
fn branch_credit_scales_with_test_branch_evidence() {
    let tmp = tempfile::TempDir::new().unwrap();
    let test_src = tmp.path().join("tests.rs");
    std::fs::write(
        &test_src,
        "fn covers_paths(flag: bool) { if flag { left(); } else { right(); } }\n",
    )
    .unwrap();
    let parsed_test = parse_rust_file(&test_src).unwrap();
    let parsed_by_path = std::collections::HashMap::from([(test_src.clone(), &parsed_test)]);
    let covering = vec![(test_src, "covers_paths".to_string())];
    let metrics = crate::rust_fn_metrics::RustFunctionMetrics {
        statements: 1,
        arguments: 0,
        max_indentation: 0,
        nested_function_depth: 0,
        returns: 0,
        branches: 3,
        local_variables: 0,
        bool_parameters: 0,
        attributes: 0,
        calls: 0,
    };

    let credit = rs_branch_credit(&metrics, true, &covering, &parsed_by_path);

    assert!(credit > 0.0 && credit < 1.0);
    assert_eq!(
        rs_branch_credit(&metrics, false, &covering, &parsed_by_path),
        0.0
    );
}

#[test]
fn weighted_mass_records_zero_and_partial_credit() {
    let file = std::path::PathBuf::from("src/lib.rs");
    let def = RustCodeDefinition {
        name: "run".to_string(),
        kind: crate::units::CodeUnitKind::Function,
        file: file.clone(),
        line: 1,
        impl_for_type: None,
    };
    let mut by_file = std::collections::HashMap::new();

    accumulate_rs_weighted_mass(&mut by_file, &def, 4, 0.0);
    accumulate_rs_weighted_mass(&mut by_file, &def, 6, 0.5);

    assert_eq!(by_file.get(&file), Some(&(3.0, 10.0)));
}

#[test]
fn locate_helpers_return_none_for_missing_or_external_items() {
    let def = RustCodeDefinition {
        name: "missing".to_string(),
        kind: crate::units::CodeUnitKind::Function,
        file: std::path::PathBuf::from("src/lib.rs"),
        line: 99,
        impl_for_type: None,
    };
    let external_mod: syn::Item = syn::parse_str("mod external;").unwrap();

    assert!(locate_in_items(&[], &def).is_none());
    assert!(locate_in_item(&external_mod, &def).is_none());
}

#[test]
fn module_import_surface_credit_is_nonzero_with_branchy_covering_test() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("lib.rs");
    let test_src = tmp.path().join("tests.rs");
    std::fs::write(
        &src,
        "pub fn target() -> u32 { 1 }\npub fn sibling() -> u32 { 2 }\n",
    )
    .unwrap();
    std::fs::write(
        &test_src,
        "fn covers(flag: bool) { if flag { target(); } else { sibling(); } }\n",
    )
    .unwrap();
    let parsed = parse_rust_file(&src).unwrap();
    let parsed_test = parse_rust_file(&test_src).unwrap();
    let refs: Vec<_> = [&parsed].into_iter().collect();
    let analysis = analyze_rust_test_refs(&refs, None);
    let target = analysis
        .definitions
        .iter()
        .find(|d| d.name == "target")
        .expect("target");
    let metrics = crate::rust_fn_metrics::RustFunctionMetrics {
        statements: 1,
        arguments: 0,
        max_indentation: 0,
        nested_function_depth: 0,
        returns: 1,
        branches: 0,
        local_variables: 0,
        bool_parameters: 0,
        attributes: 0,
        calls: 0,
    };
    let parsed_by_path = std::collections::HashMap::from([(test_src.clone(), &parsed_test)]);
    let covering = vec![(test_src, "covers".to_string())];

    let credit =
        rs_module_import_surface_credit(&analysis, target, &metrics, &covering, &parsed_by_path);

    assert!(credit.is_some_and(|v| v > 0.0 && v <= 1.0));
}
