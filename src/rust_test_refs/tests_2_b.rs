use crate::rust_test_refs::*;
use crate::rust_parsing::parse_rust_file;

#[test]
fn test_is_coverage_map_single_crate_cli_and_derive_shims() {
    use crate::rust_test_refs::calibration_map;
    assert!(calibration_map::is_coverage_map_single_crate_cli_file(Path::new(
        "src/cli/learn.rs"
    )));
    assert!(!calibration_map::is_coverage_map_single_crate_cli_file(Path::new(
        "crates/app/src/cli/foo.rs"
    )));
    assert!(calibration_map::is_coverage_map_rule_rules_mod_file(Path::new(
        "crates/ruff_linter/src/rules/flake8_bandit/rules/mod.rs"
    )));
    assert!(!calibration_map::is_coverage_map_rule_rules_mod_file(Path::new(
        "crates/ruff_linter/src/rules/flake8_bandit/mod.rs"
    )));
    assert!(calibration_map::is_coverage_map_derive_shim_file(Path::new(
        "crates/ruff_text_size/src/serde_impls.rs"
    )));
    assert!(calibration_map::is_coverage_map_derive_shim_file(Path::new(
        "crates/foo/src/parenthesize.rs"
    )));
    assert!(!calibration_map::is_coverage_map_derive_shim_file(Path::new(
        "src/lib.rs"
    )));
}

#[test]
fn test_is_coverage_map_cli_exit_shim() {
    use crate::rust_test_refs::calibration_map;
    assert!(calibration_map::is_coverage_map_cli_exit_shim(Path::new(
        "src/cli/exit.rs"
    )));
    assert!(!calibration_map::is_coverage_map_cli_exit_shim(Path::new(
        "src/cli/main.rs"
    )));
}

#[test]
fn test_is_coverage_map_json_omitted_crate() {
    use crate::rust_test_refs::calibration_map;
    assert!(calibration_map::is_coverage_map_json_omitted_crate(Path::new(
        "crates/ruff_server/src/lib.rs"
    )));
    assert!(!calibration_map::is_coverage_map_json_omitted_crate(Path::new(
        "crates/core/logger.rs"
    )));
}

#[test]
fn test_coverage_map_excluded_file_public_api() {
    assert!(coverage_map_excluded_file(Path::new(
        "src/cli/exit.rs"
    )));
    assert!(coverage_map_excluded_file(Path::new(
        "crates/ty_ide/src/lib.rs"
    )));
    assert!(coverage_map_excluded_file(Path::new(
        "crates/ruff_linter/src/rules/flake8_fixme/settings.rs"
    )));
    assert!(coverage_map_excluded_file(Path::new(
        "crates/ruff_linter/src/rules/flake8_executable/rules/mod.rs"
    )));
    assert!(coverage_map_excluded_file(Path::new(
        "crates/ruff_text_size/src/serde_impls.rs"
    )));
    assert!(!coverage_map_excluded_file(Path::new("src/lib.rs")));
}

#[test]
fn test_is_coverage_map_linter_rule_impl_file() {
    use crate::rust_test_refs::calibration_map;
    assert!(calibration_map::is_coverage_map_linter_rule_impl_file(Path::new(
        "crates/ruff_linter/src/rules/flake8_bandit/rules/unsafe_markup_use.rs"
    )));
    assert!(!calibration_map::is_coverage_map_linter_rule_impl_file(Path::new(
        "crates/ruff_linter/src/rules/flake8_bandit/mod.rs"
    )));
    assert!(!calibration_map::is_coverage_map_linter_rule_impl_file(Path::new(
        "crates/ruff_linter/src/rules/flake8_bandit/settings.rs"
    )));
}

#[test]
fn test_coverage_map_excludes_logger_even_when_referenced() {
    let def = RustCodeDefinition {
        name: "log".into(),
        kind: CodeUnitKind::Function,
        file: PathBuf::from("crates/core/logger.rs"),
        line: 1,
        end_line: 1,
        impl_for_type: None,
    };
    let refs: HashSet<String> = std::iter::once("log".into()).collect();
    assert!(!is_covered_by_tests_for_coverage_map(
        &def,
        &refs,
        &HashMap::new(),
        &HashMap::new()
    ));
}

#[test]
fn test_is_covered_by_tests_with_mode_no_impl_sibling() {
    let def = RustCodeDefinition {
        name: "m".into(),
        kind: CodeUnitKind::Method,
        file: PathBuf::from("a.rs"),
        line: 1,
        end_line: 1,
        impl_for_type: Some("T".into()),
    };
    let mut refs = HashSet::from(["T".to_string()]);
    let name_files = HashMap::new();
    let disambiguation = HashMap::new();
    assert!(!is_covered_by_tests_with_mode(
        &def, &refs, &name_files, &disambiguation, false
    ));
    refs.insert("m".into());
    assert!(is_covered_by_tests_with_mode(
        &def, &refs, &name_files, &disambiguation, false
    ));
}

#[test]
fn test_has_rust_integration_test_runner() {
    let tmp = tempfile::TempDir::new().unwrap();
    let tests_dir = tmp.path().join("tests");
    std::fs::create_dir(&tests_dir).unwrap();
    let test_rs = tests_dir.join("run.rs");
    std::fs::write(
        &test_rs,
        "fn t() { let _ = std::process::Command::new(\"kiss\"); }\n",
    )
    .unwrap();
    let lib_rs = tmp.path().join("src/lib.rs");
    std::fs::create_dir_all(lib_rs.parent().unwrap()).unwrap();
    std::fs::write(&lib_rs, "").unwrap();
    let parsed_test = parse_rust_file(&test_rs).unwrap();
    let parsed_lib = parse_rust_file(&lib_rs).unwrap();
    assert!(has_rust_integration_test_runner(&[
        &parsed_test,
        &parsed_lib
    ]));
    assert!(!has_rust_integration_test_runner(&[&parsed_lib]));
}

#[test]
fn test_coverage_map_collectors() {
    use std::io::Write;
    let code = "#[test]\nfn test_it() { foo(); }";
    let ast: syn::File = syn::parse_str(code).unwrap();
    if let syn::Item::Fn(f) = &ast.items[0] {
        let cal = references::collect_rust_references_for_fn_coverage_map(f);
        assert!(cal.contains("foo"));
    }
    let cal_usage = references::collect_per_test_usage_for_coverage_map(&ast);
    assert_eq!(cal_usage.len(), 1);
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    write!(tmp, "{code}").unwrap();
    let parsed = parse_rust_file(tmp.path()).unwrap();
    assert_eq!(references::rust_test_functions_in(&parsed), vec!["test_it"]);
    let mod_code = "#[cfg(test)] mod tests { use super::*; fn call_foo() { foo(); } }";
    let mod_ast: syn::File = syn::parse_str(mod_code).unwrap();
    let mut cal_refs = HashSet::new();
    definitions::collect_test_module_references_for_coverage_map(&mod_ast, &mut cal_refs);
    assert!(cal_refs.contains("foo"));
    let mut full_refs = HashSet::new();
    definitions::collect_test_module_references(&mod_ast, &mut full_refs);
    assert!(full_refs.contains("foo"));
    let inline = "#[test] fn inline_t() { bar(); }";
    let inline_ast: syn::File = syn::parse_str(inline).unwrap();
    let mut inline_refs = HashSet::new();
    definitions::collect_test_module_references(&inline_ast, &mut inline_refs);
    assert!(inline_refs.contains("bar"));
}

#[test]
fn test_trivial_delegation_helpers() {
    assert!(definitions::is_qualified_or_known_call(
        &syn::parse_str("module::func()").unwrap()
    ));
    assert!(definitions::is_trivial_stmt(
        &syn::parse_str::<syn::Stmt>("Ok(());").unwrap()
    ));
}

#[test]
fn test_is_binary_entry_point() {
    assert!(definitions::is_binary_entry_point(Path::new("src/main.rs")));
    assert!(definitions::is_binary_entry_point(Path::new("main.rs")));
    assert!(definitions::is_binary_entry_point(Path::new(
        "src/bin/foo.rs"
    )));
    assert!(definitions::is_binary_entry_point(Path::new(
        "legacy_tests/src/main.rs",
    )));
    assert!(!definitions::is_binary_entry_point(Path::new("src/lib.rs")));
    assert!(!definitions::is_binary_entry_point(Path::new(
        "tests/main.rs"
    )));
}

#[test]
fn test_trivial_binary_main_detection() {
    fn check(code: &str, path: &str, expect_trivial: bool, msg: &str) {
        let ast: syn::File = syn::parse_str(code).unwrap();
        if let syn::Item::Fn(f) = &ast.items[0] {
            assert_eq!(
                definitions::is_trivial_binary_main(f, Path::new(path)),
                expect_trivial,
                "{msg}"
            );
        }
    }
    check(
        "fn main() { lib::run(); }",
        "src/main.rs",
        true,
        "qualified call",
    );
    check(
        "fn main() -> Result<(), E> { lib::run()?; Ok(()) }",
        "main.rs",
        true,
        "? operator",
    );
    check(
        "fn main() { if let Err(e) = lib::run() { std::process::exit(1); } }",
        "main.rs",
        true,
        "error handling",
    );
    check(
        "fn main() { run(); }",
        "src/main.rs",
        false,
        "unqualified call",
    );
    check("fn main() { fn h() {} h(); }", "main.rs", false, "local fn");
    check(
        "fn main() { lib::run(); }",
        "src/lib.rs",
        false,
        "not entry point",
    );
    // Macro bodies are not analyzed; a `main` that only contains macros is not necessarily
    // a thin delegate and should still count as a definition for test-reference coverage.
    check(
        "fn main() { println!(\"hello\"); }",
        "src/main.rs",
        false,
        "macro-only body",
    );
}
