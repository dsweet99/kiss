use crate::common::parse_python_source;
use kiss::analyze_file;
use kiss::config::Config;
use std::fmt::Write as _;
#[test]
fn test_lines_per_file_violation() {
    let mut lines = String::new();
    for i in 0..35 {
        let _ = writeln!(&mut lines, "# line {i}");
    }
    let parsed = parse_python_source(&lines);
    let config = Config {
        lines_per_file: 20,
        ..Config::python_defaults()
    };
    let violations = analyze_file(&parsed, &config);
    assert!(
        violations.iter().any(|v| v.metric == "lines_per_file"),
        "should trigger lines_per_file violation"
    );
}

#[test]
fn test_methods_per_class_violation() {
    let code = r"
class BigClass:
    def m1(self): pass
    def m2(self): pass
    def m3(self): pass
    def m4(self): pass
    def m5(self): pass
    def m6(self): pass
";
    let parsed = parse_python_source(code);
    let config = Config {
        methods_per_class: 3,
        ..Config::python_defaults()
    };

    let violations = analyze_file(&parsed, &config);

    let has_violation = violations.iter().any(|v| v.metric == "methods_per_class");
    assert!(
        has_violation,
        "should trigger methods_per_class violation when class has 6 methods > threshold 3"
    );
}

#[test]
fn test_statements_per_try_block_violation() {
    let code = r"
def risky_function():
    try:
        a = 1
        b = 2
        c = 3
        d = 4
        e = 5
        f = 6
    except Exception:
        pass
";
    let parsed = parse_python_source(code);
    let config = Config {
        statements_per_try_block: 3,
        ..Config::python_defaults()
    };

    let violations = analyze_file(&parsed, &config);

    let has_violation = violations
        .iter()
        .any(|v| v.metric == "statements_per_try_block");
    assert!(
        has_violation,
        "should trigger statements_per_try_block violation when try block has 6 statements > threshold 3"
    );
}

#[test]
fn test_boolean_parameters_violation() {
    let code = r"
def func_with_flags(a=True, b=False):
    x = 1
";
    let parsed = parse_python_source(code);
    let config = Config {
        boolean_parameters: 1,
        ..Config::python_defaults()
    };

    let violations = analyze_file(&parsed, &config);

    let has_violation = violations.iter().any(|v| v.metric == "boolean_parameters");
    assert!(
        has_violation,
        "should trigger boolean_parameters violation when function has 2 boolean params > threshold 1"
    );
}

#[test]
fn test_returns_per_function_violation() {
    let code = r"
def many_returns(x):
    if x == 1:
        return 1
    if x == 2:
        return 2
    if x == 3:
        return 3
    return 0
";
    let parsed = parse_python_source(code);
    let config = Config {
        returns_per_function: 2,
        ..Config::python_defaults()
    };
    let violations = analyze_file(&parsed, &config);
    let v = violations
        .iter()
        .find(|v| v.metric == "returns_per_function")
        .expect("returns_per_function violation");
    assert_eq!(v.value, 4);
    assert_eq!(v.threshold, 2);
    assert_eq!(v.unit_name, "many_returns");
}

#[test]
fn test_annotations_per_function_violation() {
    let code = r"
def d(fn):
    return fn
@d
@d
@d
def decorated():
    pass
";
    let parsed = parse_python_source(code);
    let config = Config {
        annotations_per_function: 2,
        ..Config::python_defaults()
    };
    let violations = analyze_file(&parsed, &config);
    let v = violations
        .iter()
        .find(|v| v.metric == "annotations_per_function")
        .expect("annotations_per_function violation");
    assert_eq!(v.value, 3);
    assert_eq!(v.threshold, 2);
    assert_eq!(v.unit_name, "decorated");
}
