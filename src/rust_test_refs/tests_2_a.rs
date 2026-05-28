use crate::rust_test_refs::*;
use crate::rust_test_refs::calibration;
use crate::rust_parsing::parse_rust_file;

#[test]
fn test_impl_method_covered_when_type_referenced() {
    let tmp = tempfile::TempDir::new().unwrap();

    let alpha_path = tmp.path().join("alpha.rs");
    std::fs::write(
        &alpha_path,
        "pub struct Foo {}\nimpl Foo {\n    pub fn new() -> Self { Foo {} }\n}\n",
    )
    .unwrap();

    let beta_path = tmp.path().join("beta.rs");
    std::fs::write(
        &beta_path,
        "pub struct Bar {}\nimpl Bar {\n    pub fn new() -> Self { Bar {} }\n}\n",
    )
    .unwrap();

    let test_path = tmp.path().join("alpha_test.rs");
    std::fs::write(
        &test_path,
        "#[test]\nfn test_foo_new() { let _f = Foo::new(); }\n",
    )
    .unwrap();

    let parsed_alpha = parse_rust_file(&alpha_path).unwrap();
    let parsed_beta = parse_rust_file(&beta_path).unwrap();
    let parsed_test = parse_rust_file(&test_path).unwrap();

    let analysis = analyze_rust_test_refs(&[&parsed_alpha, &parsed_beta, &parsed_test], None);

    let uncovered: Vec<_> = analysis
        .unreferenced
        .iter()
        .map(|d| (d.name.as_str(), d.file.to_str().unwrap()))
        .collect();
    assert!(
        !analysis
            .unreferenced
            .iter()
            .any(|d| d.name == "new" && d.file == alpha_path),
        "Foo::new should be covered (test calls Foo::new()), but unreferenced: {uncovered:?}"
    );
    let cal = analyze_rust_test_refs_for_coverage_map(
        &[&parsed_alpha, &parsed_beta, &parsed_test],
        None,
    );
    assert!(
        cal.unreferenced
            .iter()
            .any(|d| d.name == "new" && d.file == beta_path),
        "coverage_map mode should not treat Bar::new as covered when only Foo::new is called"
    );
}

#[test]
fn test_insert_path_segments() {
    let path: syn::Path = syn::parse_str("foo::bar::Baz").unwrap();
    let mut refs = HashSet::new();
    references::insert_path_segments(&path, &mut refs);
    assert!(refs.contains("foo"));
    assert!(refs.contains("bar"));
    assert!(refs.contains("Baz"));
    let std_path: syn::Path = syn::parse_str("std::io::Read").unwrap();
    references::insert_path_segments(&std_path, &mut refs);
    assert!(!refs.contains("io"));
}

#[test]
fn test_collect_rust_references_for_fn_direct() {
    let code = "fn test_fn() { foo(); bar::baz(); }";
    let ast: syn::File = syn::parse_str(code).unwrap();
    if let syn::Item::Fn(f) = &ast.items[0] {
        let refs = references::collect_rust_references_for_fn(f);
        assert!(refs.contains("foo"));
    }
}

#[test]
fn test_collect_per_test_usage_from_items_direct() {
    let code = "#[cfg(test)] mod tests { #[test] fn test_it() { foo(); } }";
    let ast: syn::File = syn::parse_str(code).unwrap();
    let mut out = Vec::new();
    references::collect_per_test_usage_from_items(
        &ast.items,
        "",
        &mut out,
        references::RefWitnessMode::GATE,
    );
    assert!(!out.is_empty());
}

#[test]
fn test_visit_macro_tokens_direct() {
    let tokens: proc_macro2::TokenStream = "foo(bar)".parse().unwrap();
    let mut refs = HashSet::new();
    references::visit_macro_tokens(&tokens, &mut refs, references::RefWitnessMode::GATE);
    assert!(refs.contains("foo") || refs.contains("bar"));
}

#[test]
fn test_is_kiss_static_smoke_test_file() {
    use crate::rust_test_refs::calibration_map::is_kiss_static_smoke_test_file;

    assert!(is_kiss_static_smoke_test_file(Path::new(
        "src/cli/cli_cross_cov_kiss.rs"
    )));
    assert!(is_kiss_static_smoke_test_file(Path::new(
        "src/coverage_kiss_smoke.rs"
    )));
    assert!(!is_kiss_static_smoke_test_file(Path::new("src/lib.rs")));
}

#[test]
fn test_stem_self_named_definition_disambiguates() {
    let def = RustCodeDefinition {
        name: "bindings".into(),
        kind: CodeUnitKind::Function,
        file: PathBuf::from("crates/lint/src/analyze/bindings.rs"),
        line: 1,
        end_line: 1,
        impl_for_type: None,
    };
    let refs = HashSet::from(["bindings".to_string()]);
    let mut name_files = HashMap::new();
    name_files.insert(
        "bindings".to_string(),
        HashSet::from([
            PathBuf::from("crates/lint/src/analyze/bindings.rs"),
            PathBuf::from("crates/other/place.rs"),
        ]),
    );
    let disambiguation = HashMap::from([(
        "bindings".to_string(),
        PathBuf::from("crates/other/place.rs"),
    )]);
    assert!(is_directly_referenced(&def, &refs, &name_files, &disambiguation));
}

#[test]
fn test_cli_impl_method_requires_direct_call_witness() {
    let def = RustCodeDefinition {
        name: "report".into(),
        kind: CodeUnitKind::Method,
        file: PathBuf::from("src/cli/exit.rs"),
        line: 1,
        end_line: 1,
        impl_for_type: Some("Exit".into()),
    };
    let type_only = HashSet::from(["Exit".to_string()]);
    assert!(!is_covered_by_tests_for_coverage_map(
        &def,
        &type_only,
        &HashMap::new(),
        &HashMap::new()
    ));
    let with_call = HashSet::from(["Exit".to_string(), "report".to_string()]);
    assert!(is_covered_by_tests_for_coverage_map(
        &def,
        &with_call,
        &HashMap::new(),
        &HashMap::new()
    ));
}

#[test]
fn test_is_calibration_excluded_file() {
    assert!(calibration::is_calibration_excluded_file(Path::new(
        "src/flags/doc/foo.rs"
    )));
    assert!(calibration::is_calibration_excluded_file(Path::new(
        "crates/core/flags/complete/bash.rs"
    )));
    assert!(calibration::is_calibration_excluded_file(Path::new(
        "rust/crates/enn-py/src/py_fitter.rs"
    )));
    assert!(calibration::is_calibration_excluded_file(Path::new(
        "crates/core/logger.rs"
    )));
    assert!(!calibration::is_calibration_excluded_file(Path::new("src/lib.rs")));
}

#[test]
fn test_is_coverage_map_rule_settings_file() {
    use crate::rust_test_refs::calibration_map;
    assert!(calibration_map::is_coverage_map_rule_settings_file(Path::new(
        "crates/ruff_linter/src/rules/flake8_fixme/settings.rs"
    )));
    assert!(!calibration_map::is_coverage_map_rule_settings_file(Path::new(
        "crates/ruff_linter/src/rules/flake8_fixme/mod.rs"
    )));
}

#[test]
fn test_is_coverage_map_acp_kpop_body_shim() {
    use std::path::Path;
    assert!(calibration_map::is_coverage_map_acp_kpop_body_shim(Path::new(
        "src/acp/ops_body_kpop.rs"
    )));
    assert!(!calibration_map::is_coverage_map_acp_kpop_body_shim(Path::new(
        "src/acp/client_impl.rs"
    )));
}

