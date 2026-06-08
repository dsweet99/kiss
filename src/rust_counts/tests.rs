use super::*;
use std::io::Write;

#[test]
fn test_helpers() {
    let f: syn::File = syn::parse_str("impl Foo { fn a(&self) {} fn b(&self) {} }").unwrap();
    if let syn::Item::Impl(i) = &f.items[0] {
        assert_eq!(count_impl_methods(i), 2);
    }
    let f2: syn::File = syn::parse_str("impl MyStruct { fn a(&self) {} }").unwrap();
    if let syn::Item::Impl(i) = &f2.items[0] {
        assert_eq!(get_impl_type_name(i), Some("MyStruct".to_string()));
    }
}

#[test]
fn test_analyzer_basic() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "fn foo() {{}}").unwrap();
    let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
    assert!(analyze_rust_file(&parsed, &Config::default()).is_empty());
    let p = std::path::PathBuf::from("t.rs");
    let mut v = Vec::new();
    RustAnalyzer::new(&p, &Config::default(), &mut v)
        .analyze_item(&syn::parse_str::<syn::File>("fn foo() {}").unwrap().items[0]);
}

#[test]
fn test_analyzer_checks() {
    let p = std::path::PathBuf::from("t.rs");
    let cfg = Config {
        methods_per_class: 5,
        ..Default::default()
    };
    let mut v = Vec::new();
    RustAnalyzer::new(&p, &cfg, &mut v).check_methods_per_class(1, "S", 10);
    assert_eq!(v.len(), 1);
}

#[test]
fn analyze_skips_cfg_test_mod_for_per_function_rules() {
    let body = (0..15)
        .map(|_| "let _ = 1;")
        .collect::<Vec<_>>()
        .join("\n        ");
    let src = format!("#[cfg(test)]\nmod t {{\n    fn bloated() {{\n        {body}\n    }}\n}}\n");
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "{src}").unwrap();
    let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
    let cfg = Config {
        statements_per_function: 1,
        ..Default::default()
    };
    let viols = analyze_rust_file(&parsed, &cfg);
    assert!(
        !viols.iter().any(|v| v.metric == "statements_per_function"),
        "cfg(test) mod inner functions should not be checked: {viols:?}"
    );
}

#[test]
fn analyze_nested_mod_without_cfg_still_checked() {
    let body = (0..15)
        .map(|_| "let _ = 1;")
        .collect::<Vec<_>>()
        .join("\n        ");
    let src = format!("mod t {{\n    fn bloated() {{\n        {body}\n    }}\n}}\n");
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "{src}").unwrap();
    let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
    let cfg = Config {
        statements_per_function: 1,
        ..Default::default()
    };
    let viols = analyze_rust_file(&parsed, &cfg);
    assert!(
        viols.iter().any(|v| v.metric == "statements_per_function"),
        "expected statements_per_function violation in nested mod, got {viols:?}"
    );
}

#[test]
fn analyze_rust_file_include_rollup_merges_fragments() {
    let mut parent_tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(parent_tmp, "fn parent() {{}}").unwrap();
    let mut frag_tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(frag_tmp, "fn child() {{ let _ = 1; let _ = 2; }}").unwrap();
    let parent = crate::rust_parsing::parse_rust_file(parent_tmp.path()).unwrap();
    let frag = crate::rust_parsing::parse_rust_file(frag_tmp.path()).unwrap();
    let cfg = Config {
        statements_per_file: 1,
        ..Default::default()
    };
    let viols = analyze_rust_file_include_rollup(&parent, &[&frag], &cfg);
    assert!(
        viols.iter().any(|v| v.metric == "statements_per_file"),
        "rollup should aggregate fragment statements: {viols:?}"
    );
}

#[test]
fn analyze_rust_file_include_rollup_empty_included() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "fn foo() {{}}").unwrap();
    let parent = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
    assert!(analyze_rust_file_include_rollup(&parent, &[], &Config::default()).is_empty());
}

#[test]
fn analyze_rust_file_triggers_file_threshold_violations() {
    let body = (0..30)
        .map(|i| format!("let _ = {i};"))
        .collect::<Vec<_>>()
        .join("\n    ");
    let src = format!("fn bloated() {{\n    {body}\n}}\n");
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "{src}").unwrap();
    let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
    let cfg = Config {
        statements_per_file: 5,
        statements_per_function: 5,
        lines_per_file: 5,
        functions_per_file: 1,
        ..Default::default()
    };
    let viols = analyze_rust_file(&parsed, &cfg);
    assert!(
        viols.iter().any(|v| v.metric == "statements_per_file"),
        "expected file statement violation: {viols:?}"
    );
    assert!(
        viols.iter().any(|v| v.metric == "statements_per_function"),
        "expected function statement violation: {viols:?}"
    );
}

#[test]
fn analyze_rust_file_triggers_type_and_import_thresholds() {
    let mut types = String::new();
    for i in 0..8 {
        types.push_str(&format!("struct S{i} {{ x: i32 }}\n"));
    }
    types.push_str("use std::collections::{HashMap, HashSet, BTreeMap, BTreeSet};\n");
    types.push_str("fn f() {}\n");
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "{types}").unwrap();
    let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
    let cfg = Config {
        concrete_types_per_file: 3,
        imported_names_per_file: 2,
        ..Default::default()
    };
    let viols = analyze_rust_file(&parsed, &cfg);
    assert!(
        viols.iter().any(|v| v.metric == "concrete_types_per_file"),
        "expected concrete type violation: {viols:?}"
    );
    assert!(
        viols.iter().any(|v| v.metric == "imported_names_per_file"),
        "expected import violation: {viols:?}"
    );
}

#[test]
fn analyze_rust_file_triggers_function_metric_violations() {
    let src = "fn bloated(a: i32, b: i32, c: i32, d: i32, bool_flag: bool) -> i32 {\n    if a > 0 { return 1; }\n    if b > 0 { return 2; }\n    if c > 0 { return 3; }\n    if d > 0 { return 4; }\n    a + b + c + d\n}\n";
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "{src}").unwrap();
    let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
    let cfg = Config {
        arguments_per_function: 2,
        returns_per_function: 2,
        branches_per_function: 2,
        ..Default::default()
    };
    let viols = analyze_rust_file(&parsed, &cfg);
    assert!(
        viols.iter().any(|v| v.metric == "arguments_per_function"),
        "expected argument violation: {viols:?}"
    );
    assert!(
        viols.iter().any(|v| v.metric == "returns_per_function"),
        "expected return violation: {viols:?}"
    );
    assert!(
        viols.iter().any(|v| v.metric == "branches_per_function"),
        "expected branch violation: {viols:?}"
    );
}

#[test]
fn analyze_rust_file_triggers_remaining_function_metrics() {
    let src = r#"fn messy(flag: bool, other: bool) {
    let a = 1;
    let b = 2;
    let c = 3;
    let d = 4;
    if flag {
        if other {
            std::process::exit(0);
        }
    }
    foo();
    bar();
    baz();
}
fn foo() {}
fn bar() {}
fn baz() {}
"#;
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "{src}").unwrap();
    let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
    let cfg = Config {
        local_variables_per_function: 2,
        calls_per_function: 1,
        max_indentation_depth: 1,
        functions_per_file: 2,
        ..Default::default()
    };
    let viols = analyze_rust_file(&parsed, &cfg);
    assert!(
        viols.iter().any(|v| v.metric == "local_variables_per_function"),
        "expected local var violation: {viols:?}"
    );
    assert!(
        viols.iter().any(|v| v.metric == "calls_per_function"),
        "expected calls violation: {viols:?}"
    );
    assert!(
        viols.iter().any(|v| v.metric == "max_indentation_depth"),
        "expected indentation violation: {viols:?}"
    );
    assert!(
        viols.iter().any(|v| v.metric == "functions_per_file"),
        "expected functions per file violation: {viols:?}"
    );
}
