use kiss::config::Config;
use kiss::rust_counts::analyze_rust_file;
use kiss::rust_parsing::parse_rust_file;
use std::fmt::Write as _;
use std::io::Write;

#[test]
fn test_statements_per_file_violation() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    let mut code = String::from("fn big_fn() {\n");
    for i in 0..50 {
        let _ = writeln!(code, "    let x{i} = {i};");
    }
    code.push_str("}\n");
    write!(tmp, "{code}").unwrap();
    let parsed = parse_rust_file(tmp.path()).unwrap();
    let config = Config {
        statements_per_file: 10,
        ..Default::default()
    };
    let violations = analyze_rust_file(&parsed, &config);
    let has_violation = violations.iter().any(|v| v.metric == "statements_per_file");
    assert!(
        has_violation,
        "should trigger statements_per_file violation"
    );
}

#[test]
fn test_types_per_file_violation() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        tmp,
        "struct A {{}}\nstruct B {{}}\nstruct C {{}}\nstruct D {{}}\nstruct E {{}}"
    )
    .unwrap();
    let parsed = parse_rust_file(tmp.path()).unwrap();
    let config = Config {
        concrete_types_per_file: 2,
        ..Default::default()
    };
    let violations = analyze_rust_file(&parsed, &config);
    let has_violation = violations
        .iter()
        .any(|v| v.metric == "concrete_types_per_file");
    assert!(
        has_violation,
        "should trigger concrete_types_per_file violation"
    );
}

#[test]
fn test_imported_names_per_file_violation() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        tmp,
        "use std::io;\nuse std::fs;\nuse std::path;\nuse std::env;\nuse std::collections;"
    )
    .unwrap();
    let parsed = parse_rust_file(tmp.path()).unwrap();
    let config = Config {
        imported_names_per_file: 2,
        ..Default::default()
    };
    let violations = analyze_rust_file(&parsed, &config);
    let has_violation = violations
        .iter()
        .any(|v| v.metric == "imported_names_per_file");
    assert!(
        has_violation,
        "should trigger imported_names_per_file violation"
    );
}

#[test]
fn test_statements_per_function_violation() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    let mut code = String::from("fn big_fn() {\n");
    for i in 0..30 {
        let _ = writeln!(code, "    let x{i} = {i};");
    }
    code.push_str("}\n");
    write!(tmp, "{code}").unwrap();
    let parsed = parse_rust_file(tmp.path()).unwrap();
    let config = Config {
        statements_per_function: 10,
        ..Default::default()
    };
    let violations = analyze_rust_file(&parsed, &config);
    let has_violation = violations
        .iter()
        .any(|v| v.metric == "statements_per_function");
    assert!(
        has_violation,
        "should trigger statements_per_function violation"
    );
}

#[test]
fn test_arguments_per_function_violation() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        tmp,
        "fn many_args(a: i32, b: i32, c: i32, d: i32, e: i32, f: i32, g: i32, h: i32) {{}}"
    )
    .unwrap();
    let parsed = parse_rust_file(tmp.path()).unwrap();
    let config = Config {
        arguments_per_function: 3,
        ..Default::default()
    };
    let violations = analyze_rust_file(&parsed, &config);
    let has_violation = violations
        .iter()
        .any(|v| v.metric == "arguments_per_function");
    assert!(
        has_violation,
        "should trigger arguments_per_function violation"
    );
}

#[test]
fn test_max_indentation_depth_violation() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        tmp,
        r"
fn deeply_nested() {{
    if true {{
        if true {{
            if true {{
                if true {{
                    if true {{
                        let x = 1;
                    }}
                }}
            }}
        }}
    }}
}}
"
    )
    .unwrap();
    let parsed = parse_rust_file(tmp.path()).unwrap();
    let config = Config {
        max_indentation_depth: 2,
        ..Default::default()
    };
    let violations = analyze_rust_file(&parsed, &config);
    let has_violation = violations
        .iter()
        .any(|v| v.metric == "max_indentation_depth");
    assert!(
        has_violation,
        "should trigger max_indentation_depth violation"
    );
}

#[test]
fn test_returns_per_function_violation() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        tmp,
        r"
fn many_returns(x: i32) -> i32 {{
    if x == 1 {{ return 1; }}
    if x == 2 {{ return 2; }}
    if x == 3 {{ return 3; }}
    if x == 4 {{ return 4; }}
    if x == 5 {{ return 5; }}
    if x == 6 {{ return 6; }}
    0
}}
"
    )
    .unwrap();
    let parsed = parse_rust_file(tmp.path()).unwrap();
    let config = Config {
        returns_per_function: 2,
        ..Default::default()
    };
    let violations = analyze_rust_file(&parsed, &config);
    let has_violation = violations
        .iter()
        .any(|v| v.metric == "returns_per_function");
    assert!(
        has_violation,
        "should trigger returns_per_function violation"
    );
}

#[test]
fn test_branches_per_function_violation() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        tmp,
        r"
fn many_branches(x: i32) {{
    if x == 1 {{ }}
    if x == 2 {{ }}
    if x == 3 {{ }}
    if x == 4 {{ }}
    if x == 5 {{ }}
    if x == 6 {{ }}
    if x == 7 {{ }}
    if x == 8 {{ }}
}}
"
    )
    .unwrap();
    let parsed = parse_rust_file(tmp.path()).unwrap();
    let config = Config {
        branches_per_function: 3,
        ..Default::default()
    };
    let violations = analyze_rust_file(&parsed, &config);
    let has_violation = violations
        .iter()
        .any(|v| v.metric == "branches_per_function");
    assert!(
        has_violation,
        "should trigger branches_per_function violation"
    );
}

#[test]
fn test_local_variables_per_function_violation() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    let mut code = String::from("fn many_vars() {\n");
    for i in 0..25 {
        let _ = writeln!(code, "    let var{i} = {i};");
    }
    code.push_str("}\n");
    write!(tmp, "{code}").unwrap();
    let parsed = parse_rust_file(tmp.path()).unwrap();
    let config = Config {
        local_variables_per_function: 5,
        ..Default::default()
    };
    let violations = analyze_rust_file(&parsed, &config);
    let has_violation = violations
        .iter()
        .any(|v| v.metric == "local_variables_per_function");
    assert!(
        has_violation,
        "should trigger local_variables_per_function violation"
    );
}

#[test]
fn test_nested_closure_depth_violation() {
    let mut tmp = tempfile::NamedTempFile::with_suffix(".rs").unwrap();
    writeln!(
        tmp,
        r"
fn nested_closures() {{
    let f1 = || {{
        let f2 = || {{
            let f3 = || {{
                let f4 = || {{
                    1
                }};
            }};
        }};
    }};
}}
"
    )
    .unwrap();
    let parsed = parse_rust_file(tmp.path()).unwrap();
    let config = Config {
        nested_function_depth: 1,
        ..Default::default()
    };
    let violations = analyze_rust_file(&parsed, &config);
    let has_violation = violations
        .iter()
        .any(|v| v.metric == "nested_function_depth");
    assert!(
        has_violation,
        "should trigger nested_function_depth violation"
    );
}
