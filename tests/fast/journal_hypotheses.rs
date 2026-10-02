use crate::common::{first_function_node, parse_python_source};
use kiss::parsing::{ParsedFile, create_parser, parse_file};
use kiss::py_metrics::compute_function_metrics;
use std::path::Path;
use tree_sitter::Node;

fn parse_py(path: &Path) -> ParsedFile {
    let mut parser = create_parser().expect("parser should initialize");
    parse_file(&mut parser, path).expect("should parse fixture")
}

fn first_function(p: &ParsedFile) -> Node<'_> {
    first_function_node(p)
}

#[test]
fn hypothesis_01_import_not_a_statement() {
    let p = parse_python_source("def f():\n    import os\n    x = 1\n    return x\n");
    let func = p.tree.root_node().child(0).unwrap();
    let m = compute_function_metrics(func, &p.source);
    assert_eq!(m.statements, 2, "expected assignment + return only");
}

#[test]
fn hypothesis_02_type_checking_imports_excluded_from_file_metrics() {
    let p = parse_python_source(
        "from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    import os\nimport json\n",
    );
    let m = kiss::compute_file_metrics(&p);
    assert_eq!(m.imports, 2, "expected typing + json only");
}

#[test]
fn hypothesis_03_return_expression_list_counts_values() {
    let p = parse_python_source("def f():\n    return a, b, c\n");
    let func = p.tree.root_node().child(0).unwrap();
    let m = compute_function_metrics(func, &p.source);
    assert_eq!(m.max_return_values, 3);
}

#[test]
fn hypothesis_04_from_import_counts_imported_symbols() {
    let p = parse_python_source("from typing import Any, List\n");
    let m = kiss::compute_file_metrics(&p);
    assert_eq!(m.imports, 2);
}

#[test]
fn hypothesis_05_typed_boolean_defaults_counted() {
    let p = parse_python_source("def f(a: bool = True, b: int = 5):\n    return a\n");
    let func = p.tree.root_node().child(0).unwrap();
    let m = compute_function_metrics(func, &p.source);
    assert_eq!(m.boolean_parameters, 1);
}

#[test]
fn hypothesis_06_decorator_counting() {
    let p = parse_python_source("@a\n@b\n@c\ndef f():\n    return 1\n");

    let func = first_function(&p);
    let m = compute_function_metrics(func, &p.source);
    assert_eq!(m.decorators, 3);
}

#[test]
fn hypothesis_07_elif_counts_as_branch() {
    let p = parse_python_source(
        "def f(x):\n    if x:\n        return 1\n    elif x == 2:\n        return 2\n    return 3\n",
    );
    let func = p.tree.root_node().child(0).unwrap();
    let m = compute_function_metrics(func, &p.source);
    assert!(m.branches >= 2, "branches={}", m.branches);
}

#[test]
fn hypothesis_08_try_block_statements_are_try_body_only() {
    let p = parse_python_source(
        "def f():\n    try:\n        a = 1\n    except Exception:\n        b = 2\n        c = 3\n",
    );
    let func = p.tree.root_node().child(0).unwrap();
    let m = compute_function_metrics(func, &p.source);
    assert_eq!(m.max_try_block_statements, 1);
}

#[test]
fn hypothesis_09_interface_type_detection_protocol_in_args() {
    let p = parse_python_source(
        "from typing import Protocol\nclass P(Protocol, object):\n    pass\nclass C:\n    pass\n",
    );
    let m = kiss::compute_file_metrics(&p);
    assert_eq!(m.interface_types, 1);
    assert_eq!(m.concrete_types, 1);
}

#[test]
fn hypothesis_10_parenthesized_tuple_return_counts_elements() {
    let p = parse_py(Path::new("tests/fake_python/return_parenthesized_tuple.py"));
    let func = first_function(&p);
    let m = compute_function_metrics(func, &p.source);
    assert_eq!(
        m.max_return_values, 3,
        "parenthesized tuple return should count elements"
    );
}
