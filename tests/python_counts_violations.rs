use kiss::analyze_file;
use kiss::config::Config;
use kiss::parsing::{create_parser, parse_file};
use std::io::Write;

fn parse_python_source(code: &str) -> kiss::ParsedFile {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "{code}").unwrap();
    let mut parser = create_parser().unwrap();
    parse_file(&mut parser, tmp.path()).unwrap()
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
    let config = Config { methods_per_class: 3, ..Default::default() };

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
    let config = Config { statements_per_try_block: 3, ..Default::default() };

    let violations = analyze_file(&parsed, &config);

    let has_violation = violations.iter().any(|v| v.metric == "statements_per_try_block");
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
    let config = Config { boolean_parameters: 1, ..Default::default() };

    let violations = analyze_file(&parsed, &config);

    let has_violation = violations.iter().any(|v| v.metric == "boolean_parameters");
    assert!(
        has_violation,
        "should trigger boolean_parameters violation when function has 2 boolean params > threshold 1"
    );
}

