use common::{first_function_node, parse_python_source};
use kiss::parsing::{ParsedFile, create_parser, parse_file};
use kiss::py_metrics::compute_function_metrics;
use std::path::Path;
use tree_sitter::Node;
mod common;

fn parse_py(path: &Path) -> ParsedFile {
    let mut parser = create_parser().expect("parser should initialize");
    parse_file(&mut parser, path).expect("should parse fixture")
}

fn first_function(p: &ParsedFile) -> Node<'_> {
    first_function_node(p)
}

// Hypothesis 01: `import` inside a function is mistakenly counted as a statement.
// Prediction: statement count would include imports.
// Falsifying test: ensure `import` is not counted.
#[test]
fn hypothesis_01_import_not_a_statement() {
    let p = parse_python_source("def f():\n    import os\n    x = 1\n    return x\n");
    let func = p.tree.root_node().child(0).unwrap();
    let m = compute_function_metrics(func, &p.source);
    assert_eq!(m.statements, 2, "expected assignment + return only");
}

// Hypothesis 02: TYPE_CHECKING-only imports are incorrectly counted in file import metrics.
// Prediction: imports inside TYPE_CHECKING blocks would increment `imports`.
// Falsifying test: ensure those imports are excluded.
#[test]
fn hypothesis_02_type_checking_imports_excluded_from_file_metrics() {
    let p = parse_python_source(
        "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    import os\nimport json\n",
    );
    let m = kiss::compute_file_metrics(&p);
    assert_eq!(m.imports, 2, "expected typing + json only");
}

// Hypothesis 03: `return a, b, c` is not recognized as multiple return values.
// Prediction: `max_return_values` would be 1 instead of 3.
// Falsifying test: ensure it is counted as 3.
#[test]
fn hypothesis_03_return_expression_list_counts_values() {
    let p = parse_python_source("def f():\n    return a, b, c\n");
    let func = p.tree.root_node().child(0).unwrap();
    let m = compute_function_metrics(func, &p.source);
    assert_eq!(m.max_return_values, 3);
}

// Hypothesis 04: `from X import Y` incorrectly adds `X` to imported-names count (should count symbols).
// Prediction: `imports` would count X rather than Y.
// Falsifying test: `from typing import Any, List` counts 2.
#[test]
fn hypothesis_04_from_import_counts_imported_symbols() {
    let p = parse_python_source("from typing import Any, List\n");
    let m = kiss::compute_file_metrics(&p);
    assert_eq!(m.imports, 2);
}

// Hypothesis 05: boolean default parameters are missed on typed defaults.
// Prediction: `a: bool = True` would not be counted.
// Falsifying test: ensure it is counted.
#[test]
fn hypothesis_05_typed_boolean_defaults_counted() {
    let p = parse_python_source("def f(a: bool = True, b: int = 5):\n    return a\n");
    let func = p.tree.root_node().child(0).unwrap();
    let m = compute_function_metrics(func, &p.source);
    assert_eq!(m.boolean_parameters, 1);
}

// Hypothesis 06: decorators are undercounted.
// Prediction: `@a @b @c` would count < 3.
// Falsifying test: ensure it is counted as 3.
#[test]
fn hypothesis_06_decorator_counting() {
    let p = parse_python_source("@a\n@b\n@c\ndef f():\n    return 1\n");
    // In this fixture the function node is inside decorated_definition; find it.
    let func = first_function(&p);
    let m = compute_function_metrics(func, &p.source);
    assert_eq!(m.decorators, 3);
}

// Hypothesis 07: branches count ignores `elif` clauses.
// Prediction: `if/elif` would count as 1 branch.
// Falsifying test: ensure both are counted.
#[test]
fn hypothesis_07_elif_counts_as_branch() {
    let p = parse_python_source(
        "def f(x):\n    if x:\n        return 1\n    elif x == 2:\n        return 2\n    return 3\n",
    );
    let func = p.tree.root_node().child(0).unwrap();
    let m = compute_function_metrics(func, &p.source);
    assert!(m.branches >= 2, "branches={}", m.branches);
}

// Hypothesis 08: try-block statement counting includes except/finally bodies.
// Prediction: `max_try_block_statements` would reflect the largest handler, not try body.
// Falsifying test: ensure it reflects only try body.
#[test]
fn hypothesis_08_try_block_statements_are_try_body_only() {
    let p = parse_python_source(
        "def f():\n    try:\n        a = 1\n    except Exception:\n        b = 2\n        c = 3\n",
    );
    let func = p.tree.root_node().child(0).unwrap();
    let m = compute_function_metrics(func, &p.source);
    assert_eq!(m.max_try_block_statements, 1);
}

// Hypothesis 09: interface type detection fails when superclasses contain multiple args.
// Prediction: `class P(Protocol, X)` would not be classified as interface.
// Falsifying test: ensure it is.
#[test]
fn hypothesis_09_interface_type_detection_protocol_in_args() {
    let p = parse_python_source(
        "from typing import Protocol\nclass P(Protocol, object):\n    pass\nclass C:\n    pass\n",
    );
    let m = kiss::compute_file_metrics(&p);
    assert_eq!(m.interface_types, 1);
    assert_eq!(m.concrete_types, 1);
}

// Hypothesis 10 (bug candidate): `return (a, b, c)` is treated as a single return value.
// Prediction: current implementation will report `max_return_values == 1` for a parenthesized tuple.
// Test (run with `cargo test -- --ignored`): assert it should be 3; it will fail if the bug exists.
#[test]
#[ignore = "Documented historical bug; keep for reference"]
fn hypothesis_10_parenthesized_tuple_return_counts_elements() {
    let p = parse_py(Path::new("tests/fake_python/return_parenthesized_tuple.py"));
    let func = first_function(&p);
    let m = compute_function_metrics(func, &p.source);
    assert_eq!(
        m.max_return_values, 3,
        "expected parenthesized tuple to count as 3 return values"
    );
}

