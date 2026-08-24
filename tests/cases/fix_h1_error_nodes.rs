use kiss::parsing::{ParsedFile, create_parser};
use kiss::py_metrics::compute_function_metrics;

fn parse_py_result(code: &str) -> Result<ParsedFile, kiss::ParseError> {
    let mut parser = create_parser().unwrap();
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    std::io::Write::write_all(&mut tmp, code.as_bytes()).unwrap();
    kiss::parse_file(&mut parser, tmp.path())
}

fn parse_py(code: &str) -> ParsedFile {
    parse_py_result(code).unwrap()
}

fn first_func(p: &ParsedFile) -> tree_sitter::Node<'_> {
    p.tree.root_node().child(0).unwrap()
}

#[test]
fn function_with_syntax_error_sets_has_error_flag() {
    let code = "def foo():\n    x = 1\n    if True\n        y = 2\n    return x\n";
    assert!(
        parse_py_result(code).is_err(),
        "Python syntax errors must fail parse rather than yield a complete-looking tree"
    );
}

#[test]
fn function_without_syntax_error_has_no_error_flag() {
    let code = "def foo():\n    x = 1\n    if True:\n        y = 2\n    return x\n";
    let p = parse_py(code);
    let func = first_func(&p);
    let m = compute_function_metrics(func, &p.source);

    assert!(
        !m.has_error,
        "FunctionMetrics.has_error should be false for clean code"
    );
}

#[test]
fn error_functions_excluded_from_violation_counts() {
    let code = "def broken():\n    if True\n        x = 1\n";
    assert!(
        parse_py_result(code).is_err(),
        "broken Python must not enter stats as a parsed function"
    );
}
