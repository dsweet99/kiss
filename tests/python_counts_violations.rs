
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::format_push_string)]

use kiss::analyze_file;
use kiss::config::Config;
use kiss::parsing::{create_parser, parse_file};
use std::io::Write;

fn parse_source(code: &str) -> kiss::ParsedFile {
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
    let parsed = parse_source(code);
    let mut config = Config::default();
    config.methods_per_class = 3;

    let violations = analyze_file(&parsed, &config);

    let has_violation = violations.iter().any(|v| v.metric == "methods_per_class");
    assert!(
        has_violation,
        "should trigger methods_per_class violation when class has 6 methods > threshold 3"
    );
}

#[test]
fn test_lcom_violation_requires_both_methods_and_low_cohesion() {
    let mut methods = String::new();
    for i in 0..22 {
        methods.push_str(&format!("    def m{i}(self): self.field{i} = {i}\n"));
    }
    let code = format!("class LowCohesion:\n{methods}");

    let parsed = parse_source(&code);
    let mut config = Config::default();
    config.lcom = 10;

    let violations = analyze_file(&parsed, &config);

    let has_violation = violations.iter().any(|v| v.metric == "lcom");
    assert!(
        has_violation,
        "should trigger lcom violation when class has >20 methods with low cohesion"
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
    let parsed = parse_source(code);
    let mut config = Config::default();
    config.statements_per_try_block = 3;

    let violations = analyze_file(&parsed, &config);

    let has_violation = violations.iter().any(|v| v.metric == "statements_per_try_block");
    assert!(
        has_violation,
        "should trigger statements_per_try_block violation when try block has 6 statements > threshold 3"
    );
}

