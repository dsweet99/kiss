use super::*;
use std::io::Write;

#[test]
fn inline_analyzer_threshold_violations() {
    let src = (0..20)
        .map(|i| format!("let _ = {i};"))
        .collect::<Vec<_>>()
        .join("\n    ");
    let body = format!("fn bloated() {{\n    {src}\n}}\n");
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "{body}").unwrap();
    let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
    let cfg = Config {
        statements_per_file: 3,
        statements_per_function: 3,
        lines_per_file: 3,
        ..Default::default()
    };
    let viols = analyze_rust_file(&parsed, &cfg);
    assert!(!viols.is_empty(), "expected threshold violations");
}

#[test]
fn inline_build_violation_via_method_threshold() {
    let p = std::path::PathBuf::from("inline.rs");
    let cfg = Config {
        methods_per_class: 1,
        ..Default::default()
    };
    let mut v = Vec::new();
    RustAnalyzer::new(&p, &cfg, &mut v, None).check_methods_per_class(1, "S", 5);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].metric, "methods_per_class");
}

#[test]
fn inline_exhaustive_threshold_coverage() {
    let src = r#"use std::collections::{HashMap, HashSet, BTreeMap};
trait T1 {}
trait T2 {}
struct S1 { a: i32 }
struct S2 { b: i32 }
struct S3 { c: i32 }
fn f1(a: i32, b: i32, flag: bool, other: bool) -> i32 {
    let x = 1;
    let y = 2;
    let z = 3;
    if flag {
        if other {
            return x;
        }
        return y;
    }
    foo();
    bar();
    z
}
fn f2() -> i32 { 1 }
fn f3() -> i32 { 2 }
fn foo() {}
fn bar() {}
"#;
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(tmp, "{src}").unwrap();
    let parsed = crate::rust_parsing::parse_rust_file(tmp.path()).unwrap();
    let cfg = Config {
        lines_per_file: 5,
        statements_per_file: 5,
        interface_types_per_file: 1,
        concrete_types_per_file: 1,
        imported_names_per_file: 1,
        functions_per_file: 2,
        statements_per_function: 2,
        arguments_positional: 1,
        max_indentation_depth: 1,
        returns_per_function: 1,
        branches_per_function: 1,
        local_variables_per_function: 1,
        calls_per_function: 1,
        methods_per_class: 1,
        ..Default::default()
    };
    let viols = analyze_rust_file(&parsed, &cfg);
    let metrics: std::collections::HashSet<_> = viols.iter().map(|v| v.metric.as_str()).collect();
    for expected in [
        "lines_per_file",
        "statements_per_file",
        "interface_types_per_file",
        "concrete_types_per_file",
        "imported_names_per_file",
        "functions_per_file",
        "statements_per_function",
        "positional_args",
        "returns_per_function",
        "branches_per_function",
        "local_variables_per_function",
        "calls_per_function",
        "max_indentation_depth",
    ] {
        assert!(
            metrics.contains(expected),
            "missing {expected} in {metrics:?}"
        );
    }
}

#[test]
fn direct_push_file_threshold_and_build_violation() {
    let p = std::path::PathBuf::from("direct.rs");
    let cfg = Config::default();
    let mut viols = Vec::new();
    let mut analyzer = RustAnalyzer::new(&p, &cfg, &mut viols, None);
    analyzer.push_file_threshold_violation(
        "f.rs",
        "lines_per_file",
        100,
        50,
        "too long".to_string(),
        "split it",
    );
    let _built = analyzer.build_violation(3, "fn_name");
    assert_eq!(viols.len(), 1);
    assert_eq!(viols[0].metric, "lines_per_file");
}
