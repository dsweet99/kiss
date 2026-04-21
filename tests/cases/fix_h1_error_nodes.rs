use kiss::parsing::{ParsedFile, create_parser};
use kiss::py_metrics::compute_function_metrics;

fn parse_py(code: &str) -> ParsedFile {
    let mut parser = create_parser().unwrap();
    let mut tmp = tempfile::NamedTempFile::with_suffix(".py").unwrap();
    std::io::Write::write_all(&mut tmp, code.as_bytes()).unwrap();
    kiss::parse_file(&mut parser, tmp.path()).unwrap()
}

fn first_func(p: &ParsedFile) -> tree_sitter::Node<'_> {
    p.tree.root_node().child(0).unwrap()
}

/// Bug H1: kiss should not report metrics for functions whose AST contains
/// ERROR nodes. The `has_error` flag on `FunctionMetrics` should be set so
/// callers can skip unreliable results.
#[test]
fn function_with_syntax_error_sets_has_error_flag() {
    // Missing colon after `if True` — tree-sitter inserts ERROR nodes
    let code = "def foo():\n    x = 1\n    if True\n        y = 2\n    return x\n";
    let p = parse_py(code);
    let func = first_func(&p);
    let m = compute_function_metrics(func, &p.source);

    assert!(
        m.has_error,
        "FunctionMetrics.has_error should be true when AST contains ERROR nodes"
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
    use kiss::stats::MetricStats;

    let code = "def broken():\n    if True\n        x = 1\n";
    let p = parse_py(code);
    let func = first_func(&p);
    let m = compute_function_metrics(func, &p.source);

    assert!(m.has_error, "broken function should have has_error=true");

    let mut stats = MetricStats::default();
    if !m.has_error {
        stats.statements_per_function.push(m.statements);
    }
    assert!(
        stats.statements_per_function.is_empty(),
        "errored function should not contribute to stats"
    );
}
