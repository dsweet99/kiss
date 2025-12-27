use kiss::parsing::{create_parser, parse_file, ParsedFile};
use kiss::py_metrics::{
    compute_class_metrics, compute_file_metrics, compute_function_metrics,
    count_node_kind, ClassMetrics,
};
use std::io::Write;

fn parse(code: &str) -> ParsedFile {
    let mut tmp = tempfile::NamedTempFile::new().unwrap();
    write!(tmp, "{code}").unwrap();
    parse_file(&mut create_parser().unwrap(), tmp.path()).unwrap()
}

fn get_func_node(p: &ParsedFile) -> tree_sitter::Node<'_> {
    p.tree.root_node().child(0).unwrap()
}

fn get_class_node(p: &ParsedFile) -> tree_sitter::Node<'_> {
    p.tree.root_node().child(0).unwrap()
}

#[test]
fn test_function_metrics() {
    let p = parse("def f(a, b):\n    x = 1\n    return x");
    let m = compute_function_metrics(get_func_node(&p), &p.source);
    assert_eq!(m.arguments, 2);
    assert!(m.statements >= 2);
}

#[test]
fn test_class_metrics() {
    let p = parse("class C:\n    def a(self): pass\n    def b(self): pass");
    assert_eq!(compute_class_metrics(get_class_node(&p)).methods, 2);
}

#[test]
fn test_class_metrics_two_methods() {
    let p = parse("class C:\n    def a(self): self.x = 1\n    def b(self): self.x = 2");
    let m = compute_class_metrics(get_class_node(&p));
    assert_eq!(m.methods, 2);
}

#[test]
fn test_class_metrics_struct() {
    let m = ClassMetrics { methods: 5 };
    assert_eq!(m.methods, 5);
}

#[test]
fn test_file_metrics() {
    let p = parse("import os\nclass A: pass");
    let m = compute_file_metrics(&p);
    assert_eq!(m.classes, 1);
    assert!(m.imports >= 1);
}

#[test]
fn test_keyword_only_args() {
    let p = parse("def f(a, b, c, *, d, e, f): pass");
    let m = compute_function_metrics(get_func_node(&p), &p.source);
    assert_eq!(m.arguments_positional, 3);
    assert_eq!(m.arguments_keyword_only, 3);
}

#[test]
fn test_from_import_counts() {
    let p = parse("from typing import Any, List");
    assert_eq!(compute_file_metrics(&p).imports, 2);
}

#[test]
fn test_lazy_imports() {
    let p = parse("import os\ndef f():\n    import numpy");
    assert_eq!(compute_file_metrics(&p).imports, 2);
}

#[test]
fn test_try_block_statements() {
    let p = parse("def f():\n    try:\n        x=1;y=2;z=3\n    except: pass");
    assert_eq!(compute_function_metrics(get_func_node(&p), &p.source).max_try_block_statements, 3);
}

#[test]
fn test_nested_try_blocks() {
    let p = parse("def f():\n    try:\n        a=1\n    except: pass\n    try:\n        b=1;c=2;d=3;e=4\n    except: pass");
    assert_eq!(compute_function_metrics(get_func_node(&p), &p.source).max_try_block_statements, 4);
}

#[test]
fn test_boolean_parameters() {
    let p = parse("def f(a=True, b=False, c=None): pass");
    assert_eq!(compute_function_metrics(get_func_node(&p), &p.source).boolean_parameters, 2);
}

#[test]
fn test_decorators() {
    let p = parse("@a\n@b\n@c\ndef f(): pass");
    assert_eq!(compute_function_metrics(get_func_node(&p).child(3).unwrap(), &p.source).decorators, 3);
}

#[test]
fn test_count_node_kind() {
    let p = parse("def f(): pass\ndef g(): pass");
    assert_eq!(count_node_kind(p.tree.root_node(), "function_definition"), 2);
}
